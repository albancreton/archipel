#if os(macOS)
import ArchipelClient
import DaemonExample
import XCTest

final class DaemonTests: XCTestCase {
    private let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().path

    /// A short path: a socket's path may not be longer than about a hundred characters.
    private func home() -> String { NSTemporaryDirectory() + "bsw-\(UInt32.random(in: 0..<UInt32.max))" }

    func testTheDaemonIsStartedOnceAndFoundTheSecondTime() async throws {
        let archipeld = root + "/target/debug/archipeld"
        try XCTSkipUnless(FileManager.default.isExecutableFile(atPath: archipeld), "run `cargo build` first")
        let recording = root + "/testdata/transcripts/claude-v2/simple.ndjson"
        let home = home()
        defer { try? FileManager.default.removeItem(atPath: home) }

        let first = try await ensureDaemon(home: home, command: archipeld, arguments: ["--fake", recording, "--fake-speed", "0"])
        XCTAssertNotNil(first.started, "nothing was running, so it was started")
        // One answers now, so nothing is started: the program named here does not exist.
        let second = try await ensureDaemon(home: home, command: "/no/such/program")
        XCTAssertNil(second.started)

        let agents: Int = try await MainActor.run { ArchipelClient(name: "test") { try UnixSocketTransport(path: first.socket) } }.listed()
        XCTAssertEqual(agents, 1, "the daemon answers an app")

        first.stop()
        first.started?.waitUntilExit()
        XCTAssertFalse(first.started?.isRunning ?? true)
    }

    func testAStartThatFailsSaysWhy() async throws {
        let home = home()
        defer { try? FileManager.default.removeItem(atPath: home) }
        do {
            _ = try await ensureDaemon(home: home, command: "/no/such/program")
            XCTFail("there is nothing to start")
        } catch let failure as DaemonFailure {
            XCTAssertTrue(failure.description.hasPrefix("could not start the daemon"), failure.description)
        }
        // A program that starts and stops at once: its last words are in the error.
        do {
            _ = try await ensureDaemon(home: home, command: "/bin/sh", arguments: ["-c", "echo no good >&2; exit 3"])
            XCTFail("it stopped at once")
        } catch let failure as DaemonFailure {
            XCTAssertTrue(failure.description.contains("exit"), failure.description)
        }
    }
}

extension ArchipelClient {
    /// Connects, counts the agents, and closes.
    @MainActor
    fileprivate func listed() async throws -> Int {
        try await connect()
        defer { close() }
        return try await agents().count
    }
}
#endif
