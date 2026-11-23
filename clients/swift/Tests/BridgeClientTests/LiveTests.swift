import Foundation
import XCTest
@testable import BridgeClient

/// Runs the client against the real daemon program, with a fake agent that replays a recording.
/// Needs `cargo build` first: it starts `target/debug/bridged`.
@MainActor
final class LiveTests: XCTestCase {
    static let root = StateTests.testdata.deletingLastPathComponent()

    /// Starts a daemon in a fresh directory. Returns the process and its WebSocket address.
    func daemon(home: URL, scenario: String) throws -> (Process, URL) {
        let process = Process()
        process.executableURL = Self.root.appendingPathComponent("target/debug/bridged")
        process.arguments = [
            "--home", home.path, "--ws", "127.0.0.1:0", "--fake-speed", "0",
            "--fake", Self.root.appendingPathComponent("testdata/transcripts/\(scenario).ndjson").path,
        ]
        let errors = Pipe()
        process.standardError = errors
        try process.run()
        var log = ""
        while true {
            let chunk = errors.fileHandleForReading.availableData
            guard !chunk.isEmpty else { throw XCTSkip("the daemon did not start: \(log)") }
            log += String(decoding: chunk, as: UTF8.self)
            if let match = log.range(of: #"ws://[0-9.:]+(?= \(token)"#, options: .regularExpression) {
                return (process, URL(string: String(log[match]))!)
            }
        }
    }

    func testASessionDrivenThroughTheRealDaemonEndsInTheReferenceState() async throws {
        let binary = Self.root.appendingPathComponent("target/debug/bridged").path
        try XCTSkipUnless(FileManager.default.isExecutableFile(atPath: binary), "run `cargo build` first")
        let home = FileManager.default.temporaryDirectory.appendingPathComponent("bridge-swift-\(ProcessInfo.processInfo.processIdentifier)")
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        let (process, ws) = try daemon(home: home, scenario: "claude-v2/approval_denied")
        defer {
            process.interrupt()
            process.waitUntilExit()
            try? FileManager.default.removeItem(at: home)
        }

        let socket = home.appendingPathComponent("daemon.sock").path
        let app = BridgeClient(name: "swift-test") { try UnixSocketTransport(path: socket) }
        app.retryAfter = 0
        try await app.connect()
        XCTAssertEqual(app.welcome.at("_meta.bridge.scope")?.string, "admin")

        // The steps of the recording: new session, default mode, a prompt, then "No" to the approval.
        let session = try await app.newSession(cwd: "/work/probe")
        try await session.setConfigOption("mode", value: "default")
        try await session.prompt(
            "Run this exact shell command: printf 'denied fixture' > probe-denied.txt  Then reply with exactly one word saying whether it ran: yes or no.")
        await session.until { !session.pendingRequests.isEmpty }
        XCTAssertEqual(session.turnState, "requires_action")
        session.approve(session.pendingRequests[0].id, option: "reject")
        await session.until { session.turnState == "idle" }

        let expected = try JSON(parsing: String(
            contentsOf: Self.root.appendingPathComponent("testdata/daemon/claude-v2/approval_denied.state.json"), encoding: .utf8))
        XCTAssertNil(difference(session.conversation.json, expected))
        // The summary for the session list follows the update itself.
        for _ in 0..<200 where app.sessions.first?.state != "idle" {
            try await Task.sleep(nanoseconds: 5_000_000)
        }
        XCTAssertEqual(app.sessions.first?.id, session.id, "the session list was kept up to date")
        XCTAssertEqual(app.sessions.first?.state, "idle")

        // A second app, over the WebSocket, gets the same from the log.
        let token = try String(contentsOf: home.appendingPathComponent("ws-token"), encoding: .utf8)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let other = BridgeClient(name: "swift-test-2") { WebSocketTransport(url: ws, token: token) }
        other.retryAfter = 0
        try await other.connect()
        let listed = try await other.refreshSessions(loggedOnly: true)
        XCTAssertEqual(listed.map(\.id), [session.id])
        let replayed = try await other.session(session.id)
        XCTAssertNil(difference(replayed.conversation.json, expected))
        XCTAssertEqual(replayed.seq, session.seq)

        // A third app pairs with a one-time code and gets a token of its own, to observe only.
        let pairer = Process()
        pairer.executableURL = Self.root.appendingPathComponent("target/debug/bridged")
        pairer.arguments = ["--home", home.path, "pair", "--scope", "observe", "--name", "watch"]
        let printed = Pipe()
        pairer.standardOutput = printed
        pairer.standardError = Pipe()
        try pairer.run()
        pairer.waitUntilExit()
        let code = String(decoding: printed.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let paired = try await pair(url: ws, code: code)
        XCTAssertEqual([paired.scope, paired.name], ["observe", "watch"])
        let watch = BridgeClient(name: "watch") { WebSocketTransport(url: ws, token: paired.token) }
        watch.retryAfter = 0
        try await watch.connect()
        XCTAssertEqual(watch.welcome.at("_meta.bridge.scope")?.string, "observe")
        let watched = try await watch.session(session.id)
        XCTAssertNil(difference(watched.conversation.json, expected))
        do {
            try await watched.prompt("may I?")
            XCTFail("an observer may not prompt")
        } catch let error as RPCError {
            XCTAssertEqual(error.code, -32003)
        }
        watch.close()

        // An unknown session is refused.
        do {
            _ = try await other.session("no-such-session")
            XCTFail("expected a refusal")
        } catch let error as RPCError {
            XCTAssertEqual(error.code, -32602)
        }
        app.close()
        other.close()
    }
}
