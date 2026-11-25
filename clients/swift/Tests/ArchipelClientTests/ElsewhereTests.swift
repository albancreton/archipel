import XCTest
@testable import ArchipelClient

/// A daemon stand-in with one session, `s1`, that another process has open while `held` is set.
/// The real daemon's side of this is tested in Rust.
final class StandIn: Transport, @unchecked Sendable {
    private let lock = NSLock()
    private var held = true
    private let (stream, feed) = AsyncStream<JSON>.makeStream()

    func release() { lock.withLock { held = false } }

    func send(_ message: JSON) async throws {
        guard let id = message["id"], let method = message["method"]?.string else { return }
        let sessionId = message.at("params.sessionId")?.string ?? ""
        let holder: JSON? = lock.withLock { held } && sessionId == "s1" ? ["sessionId": "s1", "pid": 4242, "status": "busy"] : nil
        func say(_ frame: [String: JSON]) { feed.yield(.object(frame.merging(["jsonrpc": "2.0"]) { first, _ in first })) }
        switch method {
        case "_archipel/session/attach":
            var result: JSON = ["catchUp": "replay", "headSeq": 0]
            if let holder { result.set("openElsewhere", to: holder) }
            say(["id": id, "result": result])
            say(["method": "session/update", "params": ["sessionId": .string(sessionId), "update": ["sessionUpdate": "replay_complete"]]])
        case "session/prompt" where holder != nil:
            say(["id": id, "error": ["code": -32004, "message": "open in another process", "data": holder!]])
        case "session/fork":
            say(["id": id, "result": ["sessionId": .string(sessionId + "-fork")]])
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
final class ElsewhereTests: XCTestCase {
    func testASessionSaysWhoElseHasItOpenAndItsForkIsFreeToUse() async throws {
        let daemon = StandIn()
        let app = ArchipelClient(name: "test") { daemon }
        app.retryAfter = 0
        try await app.connect()
        let session = try await app.session("s1")
        XCTAssertEqual(session.openElsewhere, OpenElsewhere(pid: 4242, status: "busy"), "known from the attach")

        do {
            try await session.prompt("hello")
            XCTFail("the prompt is refused")
        } catch let error as RPCError {
            XCTAssertTrue(error.openElsewhere)
        }
        XCTAssertNotNil(session.openElsewhere)

        let copy = try await session.fork()
        XCTAssertEqual(copy.id, "s1-fork")
        XCTAssertNil(copy.openElsewhere)
        try await copy.prompt("hello")

        // The other process closes the session: the next prompt goes through, and it says so.
        daemon.release()
        try await session.prompt("hello")
        XCTAssertNil(session.openElsewhere)
        app.close()
    }
}
