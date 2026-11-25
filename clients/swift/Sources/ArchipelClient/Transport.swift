import Foundation

/// Something that carries JSON messages to the daemon and back.
public protocol Transport: Sendable {
    func send(_ message: JSON) async throws
    /// The next message from the daemon. Throws when the connection is gone.
    func receive() async throws -> JSON
    func close()
}

public struct ConnectionClosed: Error {}

/// The daemon's WebSocket, such as `ws://127.0.0.1:7890`, with the token for this client.
public final class WebSocketTransport: Transport, @unchecked Sendable {
    private let task: URLSessionWebSocketTask

    public init(url: URL, token: String, session: URLSession = .shared) {
        var request = URLRequest(url: url)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        task = session.webSocketTask(with: request)
        task.resume()
    }

    public func send(_ message: JSON) async throws {
        try await task.send(.string(message.text))
    }

    public func receive() async throws -> JSON {
        while true {
            if case .string(let text) = try await task.receive(), let message = try? JSON(parsing: text) {
                return message
            }
        }
    }

    public func close() {
        task.cancel(with: .normalClosure, reason: nil)
    }
}

#if canImport(Darwin)
import Darwin

/// The daemon's local socket: one JSON message per line. For apps on the same machine.
public final class UnixSocketTransport: Transport, @unchecked Sendable {
    private let handle: FileHandle
    private var lines: AsyncLineSequence<FileHandle.AsyncBytes>.AsyncIterator

    public init(path: String) throws {
        let descriptor = socket(AF_UNIX, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard path.utf8.count < capacity else {
            Darwin.close(descriptor)
            throw POSIXError(.ENAMETOOLONG)
        }
        withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            buffer.copyBytes(from: path.utf8)
        }
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else {
            let failure = errno
            Darwin.close(descriptor)
            throw POSIXError(.init(rawValue: failure) ?? .ECONNREFUSED)
        }
        handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        lines = handle.bytes.lines.makeAsyncIterator()
    }

    public func send(_ message: JSON) async throws {
        try handle.write(contentsOf: Data((message.text + "\n").utf8))
    }

    public func receive() async throws -> JSON {
        while true {
            guard let line = try await lines.next() else { throw ConnectionClosed() }
            if let message = try? JSON(parsing: line) { return message }
        }
    }

    public func close() {
        try? handle.close()
    }
}
#endif

/// What `pair` returns: the app's own token, what it may do, and the name the daemon lists it under.
public struct Pairing: Sendable, Equatable {
    public var token: String
    public var scope: String
    public var name: String
}

/// Exchanges a one-time code from `archipeld pair` for a token of the app's own. Keep the token
/// in the keychain and use it with `WebSocketTransport` from then on.
public func pair(url: URL, code: String, name: String? = nil) async throws -> Pairing {
    let transport = WebSocketTransport(url: url, token: "pair:\(code)")
    defer { transport.close() }
    var params: JSON = [:]
    if let name { params.set("name", to: .string(name)) }
    try await transport.send(["jsonrpc": "2.0", "id": 1, "method": "_archipel/pair", "params": params])
    let answer = try await transport.receive()
    guard let token = answer.at("result.token")?.string else {
        throw RPCError(code: answer.at("error.code")?.number.map(Int.init) ?? 0,
                       message: answer.at("error.message")?.string ?? "pairing failed")
    }
    return Pairing(token: token, scope: answer.at("result.scope")?.string ?? "", name: answer.at("result.name")?.string ?? "")
}
