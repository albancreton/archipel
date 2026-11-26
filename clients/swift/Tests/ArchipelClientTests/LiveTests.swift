import Foundation
import XCTest
@testable import ArchipelClient

/// Runs the client against the real daemon program, with a fake agent that replays a recording.
/// Needs `cargo build` first: it starts `target/debug/archipeld`.
@MainActor
final class LiveTests: XCTestCase {
    static let root = StateTests.testdata.deletingLastPathComponent()

    /// Starts a daemon in a fresh directory. Returns the process and its WebSocket address.
    func daemon(home: URL, scenario: String) throws -> (Process, URL) {
        let process = Process()
        process.executableURL = Self.root.appendingPathComponent("target/debug/archipeld")
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
        let binary = Self.root.appendingPathComponent("target/debug/archipeld").path
        try XCTSkipUnless(FileManager.default.isExecutableFile(atPath: binary), "run `cargo build` first")
        let home = FileManager.default.temporaryDirectory.appendingPathComponent("archipel-swift-\(ProcessInfo.processInfo.processIdentifier)")
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        let (process, ws) = try daemon(home: home, scenario: "claude-v2/approval_denied")
        defer {
            process.interrupt()
            process.waitUntilExit()
            try? FileManager.default.removeItem(at: home)
        }

        let socket = home.appendingPathComponent("daemon.sock").path
        let app = ArchipelClient(name: "swift-test") { try UnixSocketTransport(path: socket) }
        app.retryAfter = 0
        try await app.connect()
        XCTAssertEqual(app.welcome.at("_meta.archipel.scope")?.string, "admin")

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
        let other = ArchipelClient(name: "swift-test-2") { WebSocketTransport(url: ws, token: token) }
        other.retryAfter = 0
        try await other.connect()
        let listed = try await other.refreshSessions(loggedOnly: true)
        XCTAssertEqual(listed.map(\.id), [session.id])
        let replayed = try await other.session(session.id)
        XCTAssertNil(difference(replayed.conversation.json, expected))
        XCTAssertEqual(replayed.seq, session.seq)

        // A third app pairs with a one-time code and gets a token of its own, to observe only.
        let pairer = Process()
        pairer.executableURL = Self.root.appendingPathComponent("target/debug/archipeld")
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
        let watch = ArchipelClient(name: "watch") { WebSocketTransport(url: ws, token: paired.token) }
        watch.retryAfter = 0
        try await watch.connect()
        XCTAssertEqual(watch.welcome.at("_meta.archipel.scope")?.string, "observe")
        let watched = try await watch.session(session.id)
        XCTAssertNil(difference(watched.conversation.json, expected))
        do {
            try await watched.prompt("may I?")
            XCTFail("an observer may not prompt")
        } catch let error as RPCError {
            XCTAssertEqual(error.code, -32003)
        }
        watch.close()

        // The same session through a command that carries the connection, as SSH would.
        let archipel = Self.root.appendingPathComponent("target/debug/archipel").path
        let carried = ArchipelClient(name: "swift-test-3") {
            try CommandTransport(command: archipel, arguments: ["--home", home.path, "connect", "--stdio"])
        }
        carried.retryAfter = 0
        try await carried.connect()
        XCTAssertEqual(carried.welcome.at("_meta.archipel.scope")?.string, "admin")
        let throughCommand = try await carried.session(session.id)
        XCTAssertNil(difference(throughCommand.conversation.json, expected))
        carried.close()

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

    /// A daemon on another machine, through a real SSH connection. Only runs when told where:
    /// `ARCHIPEL_TEST_VIA="ssh myhost archipel connect --stdio" swift test`. It only reads.
    func testADaemonOnAnotherMachineIsReachedThroughSSH() async throws {
        guard let via = ProcessInfo.processInfo.environment["ARCHIPEL_TEST_VIA"], !via.isEmpty else {
            throw XCTSkip("set ARCHIPEL_TEST_VIA to a command that reaches a daemon")
        }
        let words = via.split(separator: " ").map(String.init)
        let app = ArchipelClient(name: "swift-ssh-test") {
            try CommandTransport(command: words[0], arguments: Array(words.dropFirst()))
        }
        app.retryAfter = 0
        try await app.connect()
        XCTAssertEqual(app.welcome.at("_meta.archipel.scope")?.string, "admin")
        _ = try await app.refreshSessions()
        app.close()
    }

    /// Lines arrive from a byte stream cut anywhere: in the middle of a message, of a character,
    /// several at once, with text that is not a message in between.
    func testAStreamOfBytesCutAnywhereGivesWholeMessages() async throws {
        let text = "Welcome to myhost\n{\"id\":1,\"text\":\"été\"}\n{\"id\":2}\n\n{\"id\":3}\n"
        let bytes = Array(text.utf8)
        for size in [1, 2, 7, bytes.count] {
            let (chunks, feed) = AsyncStream<Data>.makeStream()
            for start in stride(from: 0, to: bytes.count, by: size) {
                feed.yield(Data(bytes[start..<min(start + size, bytes.count)]))
            }
            feed.finish()
            let sent = Sent()
            let transport = StreamTransport(reading: chunks, write: { await sent.add($0) })
            var ids: [Double] = []
            var last: JSON?
            while let message = try? await transport.receive() {
                ids.append(message.at("id")?.number ?? 0)
                if last == nil { last = message }
            }
            XCTAssertEqual(ids, [1, 2, 3], "in pieces of \(size)")
            XCTAssertEqual(last?.at("text")?.string, "été")
            try await transport.send(["id": 4])
            let written = await sent.all
            XCTAssertEqual(String(decoding: written, as: UTF8.self), "{\"id\":4}\n")
        }
    }
}

private actor Sent {
    var all = Data()
    func add(_ data: Data) { all.append(data) }
}
