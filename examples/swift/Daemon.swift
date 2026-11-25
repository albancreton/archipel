// Start the daemon from a macOS app, if it is not already running.
//
// Use this when your app should work without an install step: nobody has set up `archipeld`
// as a service, so the app starts it. If a daemon already answers on the socket (a service,
// or another app that started one), that one is used and nothing is started. There is never
// a second daemon on one home folder.
//
//   let daemon = try await ensureDaemon(command: Bundle.main.path(forAuxiliaryExecutable: "archipeld")!)
//   let client = ArchipelClient(name: "my-app") { try UnixSocketTransport(path: daemon.socket) }
//   try await client.connect()
//
// Who stops it: by default the daemon this call started is stopped when the app exits in the
// ordinary way, and its sessions stop with it. An app that crashes or is killed leaves it
// running; the next start finds it and uses it. With `keep: true` it is left running on
// purpose, like a service that nobody installed; say so to your user if you do that.
//
// macOS only: an iOS app cannot start programs, and connects to a daemon on another machine
// (`docs/remote.md`).

#if os(macOS)
import ArchipelClient
import Foundation

public struct Daemon: Sendable {
    /// The daemon's folder: its socket, log and config.
    public let home: String
    public let socket: String
    /// The process, if this call started the daemon. `nil` if one was already running.
    public let started: Process?

    /// Stops the daemon if this call started it.
    public func stop() {
        guard let started, started.isRunning else { return }
        started.terminate()
    }
}

public struct DaemonFailure: Error, CustomStringConvertible {
    public let description: String
}

/// Whether a daemon answers on the socket.
private func answers(_ socket: String) -> Bool {
    guard let transport = try? UnixSocketTransport(path: socket) else { return false }
    transport.close()
    return true
}

/// The daemons to stop when the app exits.
private final class Started: @unchecked Sendable {
    static let shared = Started()
    private let lock = NSLock()
    private var processes: [Process] = []

    func add(_ process: Process) {
        lock.withLock {
            if processes.isEmpty { atexit { Started.shared.stopAll() } }
            processes.append(process)
        }
    }

    func stopAll() {
        for process in lock.withLock({ processes }) where process.isRunning { process.terminate() }
    }
}

/// The end of what the daemon wrote to stderr, to explain a failed start.
private final class Tail: @unchecked Sendable {
    private let lock = NSLock()
    private var text = ""

    func add(_ data: Data) {
        lock.withLock { text = String((text + String(decoding: data, as: UTF8.self)).suffix(2000)) }
    }

    var value: String { lock.withLock { text.trimmingCharacters(in: .whitespacesAndNewlines) } }
}

/// - Parameters:
///   - home: the daemon's folder. Default: `~/.archipel`.
///   - command: the daemon program: a path, or a name to look up in `PATH`. An app started
///     from the Finder has a bare `PATH`, so give a path there.
///   - arguments: more arguments, such as `["--fake", recording]` for a daemon that costs nothing.
///   - keep: leave the daemon running when the app exits.
///   - timeout: how long to wait for a started daemon to answer.
public func ensureDaemon(
    home: String? = nil, command: String = "archipeld", arguments: [String] = [], keep: Bool = false, timeout: TimeInterval = 10
) async throws -> Daemon {
    let home = home ?? NSHomeDirectory() + "/.archipel"
    let socket = home + "/daemon.sock"
    if answers(socket) { return Daemon(home: home, socket: socket, started: nil) }

    try FileManager.default.createDirectory(atPath: home, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    let process = Process()
    if command.contains("/") {
        process.executableURL = URL(fileURLWithPath: command)
        process.arguments = ["--home", home] + arguments
    } else {
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = [command, "--home", home] + arguments
    }
    process.standardInput = FileHandle.nullDevice
    process.standardOutput = FileHandle.nullDevice
    // The daemon logs to stderr. Keep the end of it to explain a failed start.
    let tail = Tail()
    if keep {
        process.standardError = FileHandle.nullDevice
    } else {
        let pipe = Pipe()
        pipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            if data.isEmpty { handle.readabilityHandler = nil } else { tail.add(data) }
        }
        process.standardError = pipe
    }
    do {
        try process.run()
    } catch {
        throw DaemonFailure(description: "could not start the daemon: \(error.localizedDescription)")
    }
    if !keep { Started.shared.add(process) }

    let daemon = Daemon(home: home, socket: socket, started: process)
    let deadline = Date().addingTimeInterval(timeout)
    while !answers(socket) {
        if !process.isRunning {
            throw DaemonFailure(description: "the daemon stopped at once (exit \(process.terminationStatus)): \(tail.value)")
        }
        if Date() > deadline {
            daemon.stop()
            throw DaemonFailure(description: "the daemon did not answer on \(socket)")
        }
        try await Task.sleep(nanoseconds: 50_000_000)
    }
    return daemon
}
#endif
