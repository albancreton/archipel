import ArchipelClient
import Citadel
import CryptoKit
import Foundation
import NIOSSH
import SSHExample
import XCTest

/// `StreamTransport` on a channel of a real SSH library, against a real SSH server.
///
/// The server is the system's own `sshd`, started for the test as the current user on a port of
/// loopback, with a host key and an allowed key made for the test. No setting of the machine is
/// touched, and nothing else can log in to it. Behind it, a daemon with the fake agent.
/// Needs `cargo build` at the root of the repository first.
@MainActor
final class SSHTests: XCTestCase {
    static let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()

    func testASessionIsDrivenThroughAnSSHLibrarysChannel() async throws {
        let archipeld = Self.root.appendingPathComponent("target/debug/archipeld").path
        let archipel = Self.root.appendingPathComponent("target/debug/archipel").path
        try XCTSkipUnless(FileManager.default.isExecutableFile(atPath: archipeld), "run `cargo build` first")
        try XCTSkipUnless(FileManager.default.isExecutableFile(atPath: "/usr/sbin/sshd"), "no sshd on this machine")

        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("archipel-ssh-\(ProcessInfo.processInfo.processIdentifier)")
        try? FileManager.default.removeItem(at: folder)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let home = folder.appendingPathComponent("home")

        let daemon = Process()
        daemon.executableURL = URL(fileURLWithPath: archipeld)
        daemon.arguments = [
            "--home", home.path, "--fake-speed", "0",
            "--fake", Self.root.appendingPathComponent("testdata/transcripts/claude-v2/approval_denied.ndjson").path,
        ]
        daemon.standardError = FileHandle.nullDevice
        try daemon.run()

        // The server's key, and the one key that may log in.
        let key = Curve25519.Signing.PrivateKey()
        try run("/usr/bin/ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", folder.appendingPathComponent("host_key").path])
        try (openSSH(key.publicKey) + "\n").write(to: folder.appendingPathComponent("authorized_keys"), atomically: true, encoding: .utf8)
        let port = try freePort()
        try """
            Port \(port)
            ListenAddress 127.0.0.1
            HostKey \(folder.path)/host_key
            AuthorizedKeysFile \(folder.path)/authorized_keys
            PidFile \(folder.path)/pid
            UsePAM no
            StrictModes no
            PasswordAuthentication no
            KbdInteractiveAuthentication no

            """.write(to: folder.appendingPathComponent("sshd_config"), atomically: true, encoding: .utf8)
        let server = Process()
        server.executableURL = URL(fileURLWithPath: "/usr/sbin/sshd")
        server.arguments = ["-D", "-e", "-f", folder.appendingPathComponent("sshd_config").path]
        server.standardError = FileHandle.nullDevice
        try server.run()
        defer {
            server.terminate()
            daemon.interrupt()
            daemon.waitUntilExit()
            try? FileManager.default.removeItem(at: folder)
        }
        try await untilListening(port: port)
        try await untilListening(socket: home.appendingPathComponent("daemon.sock").path)

        let hostKey = try NIOSSHPublicKey(openSSHPublicKey:
            String(contentsOf: folder.appendingPathComponent("host_key.pub"), encoding: .utf8))
        let user = NSUserName()
        func connect(trusting trusted: NIOSSHPublicKey, with key: Curve25519.Signing.PrivateKey) async throws -> SSHClient {
            try await SSHClient.connect(to: SSHClientSettings(
                host: "127.0.0.1", port: port,
                authenticationMethod: { .ed25519(username: user, privateKey: key) },
                hostKeyValidator: .trustedKeys([trusted])))
        }

        // A server that is not the one expected, and a key that is not allowed: both are SSH's to refuse.
        let another = try NIOSSHPublicKey(openSSHPublicKey: openSSH(Curve25519.Signing.PrivateKey().publicKey))
        do {
            _ = try await connect(trusting: another, with: key)
            XCTFail("a server with another key was accepted")
        } catch {}
        do {
            _ = try await connect(trusting: hostKey, with: Curve25519.Signing.PrivateKey())
            XCTFail("a key that is not allowed got in")
        } catch {}

        let ssh = try await connect(trusting: hostKey, with: key)
        let command = "\(archipel) --home \(home.path) connect --stdio"
        let app = ArchipelClient(name: "swift-ssh-example") { try await daemonTransport(over: ssh, command: command) }
        app.retryAfter = 0
        try await app.connect()
        XCTAssertEqual(app.welcome.at("_meta.archipel.scope")?.string, "admin")

        // The steps of the recording: new session, default mode, a prompt, then "No" to the approval.
        let session = try await app.newSession(cwd: "/work/probe")
        try await session.setConfigOption("mode", value: "default")
        try await session.prompt("Run this exact shell command: printf 'denied fixture' > probe-denied.txt")
        await session.until { !session.pendingRequests.isEmpty }
        XCTAssertEqual(session.turnState, "requires_action")
        session.approve(session.pendingRequests[0].id, option: "reject")
        await session.until { session.turnState == "idle" }

        // What arrived through SSH is what an app on the machine itself has.
        let socket = home.appendingPathComponent("daemon.sock").path
        let local = ArchipelClient(name: "swift-ssh-local") { try UnixSocketTransport(path: socket) }
        local.retryAfter = 0
        try await local.connect()
        let same = try await local.session(session.id)
        XCTAssertTrue(session.conversation.json == same.conversation.json, "the session differs from the one read on the machine")
        // And it is the state the recording must give.
        let expected = try JSON(parsing: String(
            contentsOf: Self.root.appendingPathComponent("testdata/daemon/claude-v2/approval_denied.state.json"), encoding: .utf8))
        XCTAssertTrue(session.conversation.json == expected, "the session differs from the reference state")
        XCTAssertEqual(session.seq, same.seq)
        XCTAssertGreaterThan(session.conversation.json.text.utf8.count, 500)

        // A second channel on the same SSH connection: no new handshake for it.
        let again = ArchipelClient(name: "swift-ssh-example-2") { try await daemonTransport(over: ssh, command: command) }
        again.retryAfter = 0
        try await again.connect()
        let listed = try await again.refreshSessions(loggedOnly: true)
        XCTAssertEqual(listed.map(\.id), [session.id])

        // A command that does not start is an error, not a wait.
        do {
            let transport = try await daemonTransport(over: ssh, command: "/nonexistent/archipel connect --stdio")
            _ = try await transport.receive()
            XCTFail("a command that does not exist gave a message")
        } catch {}

        again.close()
        local.close()
        app.close()
        try await ssh.close()
    }

    // MARK: - What the test needs around it

    /// A public key as a line of `authorized_keys`.
    func openSSH(_ key: Curve25519.Signing.PublicKey) -> String {
        func field(_ bytes: Data) -> Data {
            withUnsafeBytes(of: UInt32(bytes.count).bigEndian) { Data($0) } + bytes
        }
        let blob = field(Data("ssh-ed25519".utf8)) + field(key.rawRepresentation)
        return "ssh-ed25519 \(blob.base64EncodedString()) test"
    }

    func run(_ program: String, _ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: program)
        process.arguments = arguments
        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, program)
    }

    /// A port of loopback nothing listens on.
    func freePort() throws -> Int {
        let descriptor = socket(AF_INET, SOCK_STREAM, 0)
        defer { close(descriptor) }
        var address = sockaddr_in()
        address.sin_family = sa_family_t(AF_INET)
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let bound = withUnsafeMutablePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { pointer in
                Darwin.bind(descriptor, pointer, length) == 0 && getsockname(descriptor, pointer, &length) == 0
            }
        }
        guard bound else { throw POSIXError(.EADDRNOTAVAIL) }
        return Int(UInt16(bigEndian: address.sin_port))
    }

    func untilListening(port: Int) async throws {
        for _ in 0..<200 {
            let descriptor = socket(AF_INET, SOCK_STREAM, 0)
            defer { close(descriptor) }
            var address = sockaddr_in()
            address.sin_family = sa_family_t(AF_INET)
            address.sin_port = UInt16(port).bigEndian
            address.sin_addr.s_addr = inet_addr("127.0.0.1")
            let connected = withUnsafePointer(to: &address) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) == 0
                }
            }
            if connected { return }
            try await Task.sleep(nanoseconds: 25_000_000)
        }
        XCTFail("nothing listens on port \(port)")
    }

    func untilListening(socket path: String) async throws {
        for _ in 0..<200 {
            if let transport = try? UnixSocketTransport(path: path) {
                transport.close()
                return
            }
            try await Task.sleep(nanoseconds: 25_000_000)
        }
        XCTFail("the daemon did not start")
    }
}
