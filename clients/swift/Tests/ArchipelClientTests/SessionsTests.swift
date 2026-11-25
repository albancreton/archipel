import XCTest
@testable import ArchipelClient

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
        case "_archipel/session/attach":
            say(["id": id, "result": ["catchUp": "replay", "headSeq": 0]])
            say(["method": "session/update", "params": ["sessionId": message.at("params.sessionId") ?? .null, "update": ["sessionUpdate": "replay_complete"]]])
        case "session/options":
            say(["id": id, "result": ["configOptions": [["id": "model"]]]])
        case "session/set_title":
            say(["id": id, "result": ["title": "tidy"]])
        case "_archipel/sessions/list":
            say(["id": id, "result": ["sessions": [["sessionId": "s1", "agentId": "claude", "cwd": "/work", "live": false, "logged": true, "headSeq": 0]]]])
        case "_archipel/agents/list":
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
        let app = ArchipelClient(name: "test") { daemon }
        app.retryAfter = 0
        try await app.connect()

        let fresh = try await app.sessionOptions(cwd: "/work", agentId: "pi")
        XCTAssertEqual(fresh["configOptions"]?.array?.count, 1)
        XCTAssertEqual(daemon.sent.last, ["method": "session/options", "params": ["cwd": "/work", "_meta": ["archipel": ["agentId": "pi"]]]])
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
        let attaches = { daemon.sent.filter { $0["method"]?.string == "_archipel/session/attach" }.count }
        XCTAssertEqual(attaches(), 1)
        _ = try await app.session("s1")
        XCTAssertEqual(attaches(), 2)
        app.close()
    }
}

/// What a stand-in daemon knows, shared by the connections an app opens to it one after another.
final class Known: @unchecked Sendable {
    private let lock = NSLock()
    private var sessions: Set<String>
    private var current: Line?

    init(_ sessions: Set<String>) { self.sessions = sessions }

    func has(_ id: String) -> Bool { lock.withLock { sessions.contains(id) } }
    func forget(_ id: String) { lock.withLock { _ = sessions.remove(id) } }
    func open() -> Line {
        let line = Line(self)
        lock.withLock { current = line }
        return line
    }
    /// Says something on the current connection.
    func say(_ frame: JSON) { lock.withLock { current }?.push(frame) }
    /// Drops the current connection.
    func drop() { lock.withLock { current }?.close() }
}

/// One connection to a `Known` daemon.
final class Line: Transport, @unchecked Sendable {
    private let known: Known
    private let (stream, feed) = AsyncStream<JSON>.makeStream()

    init(_ known: Known) { self.known = known }

    func push(_ frame: JSON) { feed.yield(frame) }

    func send(_ message: JSON) async throws {
        guard let id = message["id"], let method = message["method"]?.string else { return }
        let sessionId = message.at("params.sessionId") ?? .null
        guard method == "_archipel/session/attach" else { return push(["jsonrpc": "2.0", "id": id, "result": [:]]) }
        guard known.has(sessionId.string ?? "") else {
            return push(["jsonrpc": "2.0", "id": id, "error": ["code": -32602, "message": "unknown session; list the sessions first"]])
        }
        push(["jsonrpc": "2.0", "id": id, "result": ["catchUp": "replay", "headSeq": 0]])
        push(["jsonrpc": "2.0", "method": "session/update", "params": ["sessionId": sessionId, "update": ["sessionUpdate": "replay_complete"]]])
    }

    func receive() async throws -> JSON {
        for await message in stream { return message }
        throw ConnectionClosed()
    }

    func close() { feed.finish() }
}

@MainActor
final class RemovedTests: XCTestCase {
    func testASessionAnotherAppDeletedIsMarkedStopsWaitingAndIsNoLongerFollowed() async throws {
        let daemon = Known(["s1", "s2"])
        let app = ArchipelClient(name: "test") { daemon.open() }
        app.retryAfter = 0
        try await app.connect()
        let session = try await app.session("s1")
        let other = try await app.session("s2")

        daemon.say(["jsonrpc": "2.0", "method": "_archipel/sessions/removed", "params": ["sessionId": "s1"]])
        await session.until { session.turnState == "running" }  // nothing will change any more, so the wait ends
        XCTAssertTrue(session.removed)
        XCTAssertFalse(other.removed)
        app.close()
    }

    func testAnErrorOutsideAnyRequestIsKeptOnItsSessionUntilTheNextChangeArrives() async throws {
        let daemon = Known(["s1", "s2"])
        let app = ArchipelClient(name: "test") { daemon.open() }
        try await app.connect()
        let session = try await app.session("s1")
        let other = try await app.session("s2")

        daemon.say(["jsonrpc": "2.0", "method": "_archipel/session/error",
                    "params": ["sessionId": "s1", "code": -32005, "message": "the disk is full"]])
        await session.until { session.error != nil }
        XCTAssertEqual(session.error?.storage, true)
        XCTAssertEqual(session.error?.message, "the disk is full")
        XCTAssertNil(other.error)
        // The log is written again: the first update that arrives clears it.
        daemon.say(["jsonrpc": "2.0", "method": "session/update", "params": [
            "sessionId": "s1", "update": ["sessionUpdate": "state_update", "state": "running"],
            "_meta": ["archipel": ["seq": 1]]]])
        await session.until { session.turnState == "running" }
        XCTAssertNil(session.error)
        app.close()
    }

    func testTheAppIsToldOnceWhenASessionStartsToNeedTheUser() async throws {
        XCTAssertEqual(Attention.of(before: nil, after: "idle"), nil)
        XCTAssertEqual(Attention.of(before: "running", after: "requires_action"), .needsAnswer)
        XCTAssertEqual(Attention.of(before: "requires_action", after: "requires_action"), nil)
        XCTAssertEqual(Attention.of(before: "requires_action", after: "idle"), .finished)
        XCTAssertEqual(Attention.of(before: nil, after: "requires_action"), .needsAnswer)

        let daemon = Known(["s1"])
        let app = ArchipelClient(name: "test") { daemon.open() }
        var heard: [String] = []
        app.onAttention = { session, why in heard.append("\(session.id) \(why)") }
        try await app.connect()
        let session = try await app.session("s1")
        for state in ["idle", "running", "requires_action", "requires_action", "running", "idle"] {
            daemon.say(["jsonrpc": "2.0", "method": "_archipel/sessions/updated", "params": ["session": [
                "sessionId": "s1", "agentId": "claude", "cwd": "/w", "state": .string(state)]]])
        }
        // Messages are handled in order: once this update has arrived, the ones before have too.
        daemon.say(["jsonrpc": "2.0", "method": "session/update", "params": [
            "sessionId": "s1", "update": ["sessionUpdate": "state_update", "state": "running"], "_meta": ["archipel": ["seq": 1]]]])
        await session.until { session.turnState == "running" }
        XCTAssertEqual(heard, ["s1 needsAnswer", "s1 finished"])
        app.close()
    }

    func testASessionDeletedWhileTheConnectionWasDownDoesNotStopTheClientComingBack() async throws {
        let daemon = Known(["s1", "s2"])
        let app = ArchipelClient(name: "test") { daemon.open() }
        app.retryAfter = 0.01
        try await app.connect()
        let deleted = try await app.session("s1")
        let kept = try await app.session("s2")

        daemon.forget("s1")
        daemon.drop()
        await deleted.until { false }  // ends when the session is marked as deleted
        XCTAssertTrue(deleted.removed)
        XCTAssertFalse(kept.removed)
        // The client is back: a call is answered.
        while !app.connected { await Task.yield() }
        _ = try await app.call("anything")
        app.close()
    }
}
