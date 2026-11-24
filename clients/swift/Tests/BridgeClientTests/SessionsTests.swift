import XCTest
@testable import BridgeClient

/// A daemon stand-in that records the calls. The real daemon's side is tested in Rust.
final class Recorder: Transport, @unchecked Sendable {
    private let lock = NSLock()
    private var calls: [JSON] = []
    private let (stream, feed) = AsyncStream<JSON>.makeStream()

    var sent: [JSON] { lock.withLock { calls } }

    func send(_ message: JSON) async throws {
        guard let id = message["id"], let method = message["method"]?.string else { return }
        lock.withLock { calls.append(["method": .string(method), "params": message["params"] ?? .null]) }
        func say(_ frame: [String: JSON]) { feed.yield(.object(frame.merging(["jsonrpc": "2.0"]) { first, _ in first })) }
        switch method {
        case "_bridge/session/attach":
            say(["id": id, "result": ["catchUp": "replay", "headSeq": 0]])
            say(["method": "session/update", "params": ["sessionId": message.at("params.sessionId") ?? .null, "update": ["sessionUpdate": "replay_complete"]]])
        case "session/options":
            say(["id": id, "result": ["configOptions": [["id": "model"]]]])
        case "session/set_title":
            say(["id": id, "result": ["title": "tidy"]])
        case "_bridge/sessions/list":
            say(["id": id, "result": ["sessions": [["sessionId": "s1", "agentId": "claude", "cwd": "/work", "live": false, "logged": true, "headSeq": 0]]]])
        case "_bridge/agents/list":
            say(["id": id, "result": ["agents": [["agentId": "claude", "capabilities": ["session": ["title": [:]]]]]]])
        default:
            say(["id": id, "result": [:]])
        }
    }

    func receive() async throws -> JSON {
        for await message in stream { return message }
        throw ConnectionClosed()
    }

    func close() { feed.finish() }
}

@MainActor
final class SessionsTests: XCTestCase {
    func testSettingsWithoutASessionRenameAndDelete() async throws {
        let daemon = Recorder()
        let app = BridgeClient(name: "test") { daemon }
        app.retryAfter = 0
        try await app.connect()

        let fresh = try await app.sessionOptions(cwd: "/work", agentId: "pi")
        XCTAssertEqual(fresh["configOptions"]?.array?.count, 1)
        XCTAssertEqual(daemon.sent.last, ["method": "session/options", "params": ["cwd": "/work", "_meta": ["bridge": ["agentId": "pi"]]]])
        _ = try await app.sessionOptions(cwd: "/work", sessionId: "s1")
        XCTAssertEqual(daemon.sent.last?["params"], ["cwd": "/work", "sessionId": "s1"])

        let stored = try await app.setTitle("s1", to: "  tidy  ")
        XCTAssertEqual(stored, "tidy", "the title the harness stored")

        let agents = try await app.agents()
        XCTAssertNotNil(agents.first?.at("capabilities.session.title"))

        // A deleted session leaves the list and is no longer followed.
        try await app.refreshSessions()
        _ = try await app.session("s1")
        XCTAssertEqual(app.sessions.map(\.id), ["s1"])
        try await app.deleteSession("s1")
        XCTAssertEqual(daemon.sent.last, ["method": "session/delete", "params": ["sessionId": "s1"]])
        XCTAssertTrue(app.sessions.isEmpty)
        let attaches = { daemon.sent.filter { $0["method"]?.string == "_bridge/session/attach" }.count }
        XCTAssertEqual(attaches(), 1)
        _ = try await app.session("s1")
        XCTAssertEqual(attaches(), 2)
        app.close()
    }
}
