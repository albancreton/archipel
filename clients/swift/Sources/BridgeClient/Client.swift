import Foundation
import Observation

/// The daemon or the agent refused a request.
public struct RPCError: Error, CustomStringConvertible, Equatable {
    public var code: Int
    public var message: String
    public var description: String { "\(message) (\(code))" }
}

/// One line of the session list.
public struct SessionSummary: Identifiable, Equatable, Sendable {
    public var id: String
    public var agentId: String
    public var title: String?
    public var cwd: String
    /// An adapter process holds the session.
    public var live: Bool
    /// The session is in the daemon's log. If not, opening it loads its history first.
    public var logged: Bool
    /// `running`, `requires_action` or `idle`.
    public var state: String?
    public var headSeq: Int
    public var updatedAt: String?

    init?(_ json: JSON) {
        guard let id = json["sessionId"]?.string else { return nil }
        self.id = id
        agentId = json["agentId"]?.string ?? ""
        title = json["title"]?.string
        cwd = json["cwd"]?.string ?? ""
        live = json["live"] == .bool(true)
        logged = json["logged"] != .bool(false)
        state = json["state"]?.string
        headSeq = json["headSeq"]?.number.map(Int.init) ?? 0
        updatedAt = json["updatedAt"]?.string
    }
}

/// The client an app uses. Its sessions stay in step with the daemon, across reconnects.
/// It is observable: SwiftUI views that read `connected` or `sessions` update by themselves.
@MainActor
@Observable
public final class BridgeClient {
    /// False while the connection is lost and being retried.
    public private(set) var connected = false
    /// The session list, kept current after `refreshSessions()`.
    public private(set) var sessions: [SessionSummary] = []
    /// What the daemon answered to `initialize`: its capabilities, and this connection's scope.
    public private(set) var welcome: JSON = .null

    private let name: String
    private let open: @Sendable () async throws -> any Transport
    /// Seconds between attempts to reconnect. 0 disables reconnecting.
    public var retryAfter: TimeInterval = 1

    private var transport: (any Transport)?
    private var nextId = 1
    private var waiting: [Int: CheckedContinuation<JSON, Error>] = [:]
    /// Attach calls under way: call id to session id. See `receive`.
    private var attaching: [Int: String] = [:]
    private var followed: [String: BridgeSession] = [:]
    private var closed = false

    /// `open` opens a transport, and is called again after a disconnect.
    public init(name: String, open: @escaping @Sendable () async throws -> any Transport) {
        self.name = name
        self.open = open
    }

    /// Connects and introduces the app to the daemon.
    public func connect() async throws {
        let transport = try await open()
        self.transport = transport
        Task { await self.readLoop(transport) }
        welcome = try await call("initialize", [
            "protocolVersion": 2,
            "info": ["name": .string(name), "version": "0.0.0"],
            "capabilities": ["elicitation": ["form": [:], "url": [:]]],
        ])
        connected = true
    }

    private func readLoop(_ transport: any Transport) async {
        while let message = try? await transport.receive() {
            receive(message)
        }
        // The connection is gone.
        guard !closed, self.transport.map({ ObjectIdentifier($0 as AnyObject) }) == ObjectIdentifier(transport as AnyObject) else { return }
        connected = false
        for continuation in waiting.values { continuation.resume(throwing: ConnectionClosed()) }
        waiting.removeAll()
        attaching.removeAll()
        guard retryAfter > 0 else { return }
        while !closed {
            try? await Task.sleep(nanoseconds: UInt64(retryAfter * 1_000_000_000))
            do {
                try await connect()
                // Every session continues from the last sequence number it applied.
                for session in followed.values { try await session.attach() }
                return
            } catch {
                continue  // not back yet
            }
        }
    }

    private func receive(_ message: JSON) {
        if message["method"] == nil, let id = message["id"]?.number.map(Int.init) {
            // An attach that announces a replay: start that session from nothing, now, before
            // the replay that follows this response is read.
            if let sessionId = attaching.removeValue(forKey: id), message.at("result.catchUp")?.string == "replay" {
                followed[sessionId]?.reset()
            }
            let continuation = waiting.removeValue(forKey: id)
            // A response may also change state (the config options in `session/new`).
            for session in followed.values { session.received(message) }
            if let error = message["error"] {
                continuation?.resume(throwing: RPCError(
                    code: error["code"]?.number.map(Int.init) ?? 0, message: error["message"]?.string ?? "refused"))
            } else {
                continuation?.resume(returning: message["result"] ?? .null)
            }
            return
        }
        if message["method"]?.string == Bridge.sessionsUpdated {
            if let summary = message.at("params.session").flatMap(SessionSummary.init) {
                if let index = sessions.firstIndex(where: { $0.id == summary.id }) {
                    sessions[index] = summary
                } else {
                    sessions.insert(summary, at: 0)
                }
            }
            return
        }
        if let root = Bridge.root(of: message) { followed[root]?.received(message) }
    }

    /// Sends any request, for example an extension's `_x/<namespace>/…` method.
    @discardableResult
    public func call(_ method: String, _ params: JSON = [:]) async throws -> JSON {
        guard let transport else { throw ConnectionClosed() }
        let id = nextId
        nextId += 1
        let message: JSON = ["jsonrpc": "2.0", "id": .number(Double(id)), "method": .string(method), "params": params]
        if method == Bridge.sessionAttach, let sessionId = params["sessionId"]?.string { attaching[id] = sessionId }
        if let root = Bridge.root(of: message) { followed[root]?.sent(message) }
        return try await withCheckedThrowingContinuation { continuation in
            waiting[id] = continuation
            Task {
                do {
                    try await transport.send(message)
                } catch {
                    self.waiting.removeValue(forKey: id)?.resume(throwing: error)
                }
            }
        }
    }

    func send(_ message: JSON) {
        guard let transport else { return }
        Task { try? await transport.send(message) }
    }

    /// Reads all sessions, newest first: the daemon's own and those each harness reports.
    /// `loggedOnly` leaves the latter out and starts no agent.
    @discardableResult
    public func refreshSessions(loggedOnly: Bool = false) async throws -> [SessionSummary] {
        let result = try await call(Bridge.sessionsList, ["loggedOnly": .bool(loggedOnly)])
        sessions = (result["sessions"]?.array ?? []).compactMap(SessionSummary.init)
        return sessions
    }

    /// Creates a session and attaches to it.
    public func newSession(cwd: String, agentId: String? = nil) async throws -> BridgeSession {
        var params: JSON = ["cwd": .string(cwd), "mcpServers": []]
        if let agentId { params.set("_meta.bridge.agentId", to: .string(agentId)) }
        let created = try await call("session/new", params)
        guard let id = created["sessionId"]?.string else { throw RPCError(code: 0, message: "the agent did not return a session id") }
        return try await session(id)
    }

    /// Attaches to a session and returns once the app holds all of it.
    public func session(_ id: String) async throws -> BridgeSession {
        if let known = followed[id] { return known }
        let session = BridgeSession(id: id, client: self)
        followed[id] = session
        do {
            try await session.attach()
        } catch {
            followed.removeValue(forKey: id)
            throw error
        }
        return session
    }

    /// Stops following a session. It keeps running in the daemon.
    public func detach(_ id: String) async {
        followed.removeValue(forKey: id)
        _ = try? await call(Bridge.sessionDetach, ["sessionId": .string(id)])
    }

    public func close() {
        closed = true
        transport?.close()
    }
}

/// A session the app follows. What it holds is always what the daemon has, up to `seq`.
/// It is observable: a SwiftUI view that reads it updates after every change.
@MainActor
@Observable
public final class BridgeSession: Identifiable {
    public nonisolated let id: String
    public private(set) var mirror = Mirror()
    @ObservationIgnored private weak var client: BridgeClient?
    @ObservationIgnored private var waiters: [(done: () -> Bool, continuation: CheckedContinuation<Void, Never>)] = []
    @ObservationIgnored private var replayEnded = false

    init(id: String, client: BridgeClient) {
        self.id = id
        self.client = client
    }

    /// Every session of the conversation: the user's and its subagents'.
    public var conversation: Conversation { mirror.conversation }
    /// The user's session. Its `timeline` lists the items in order.
    public var state: SessionState { mirror.conversation.sessions[id] ?? SessionState() }
    /// The newest sequence number applied.
    public var seq: Int { mirror.seq }
    /// `running`, `requires_action` or `idle`.
    public var turnState: String? { state.values["state"]?["state"]?.string }

    /// Approvals and questions nobody has answered, across the session and its subagents.
    public var pendingRequests: [(id: String, sessionId: String, request: Request)] {
        mirror.conversation.order.flatMap { sessionId in
            (mirror.conversation.sessions[sessionId]?.pendingRequests ?? []).map { ($0.id, sessionId, $0.request) }
        }
    }

    /// Sends a message to the agent. Returns when the agent has taken it, not when it is done.
    public func prompt(_ text: String) async throws {
        try await client?.call("session/prompt", ["sessionId": .string(id), "prompt": [["type": "text", "text": .string(text)]]])
    }

    /// Answers an approval with one of its options.
    public func approve(_ requestId: String, option: String) {
        answer(requestId, result: ["outcome": ["outcome": "selected", "optionId": .string(option)]])
    }

    /// Answers a request with a raw result, such as the form content of a question.
    public func answer(_ requestId: String, result: JSON) {
        client?.send(["jsonrpc": "2.0", "id": .string(requestId), "result": result])
    }

    /// Interrupts the running turn.
    public func cancel() {
        client?.send(["jsonrpc": "2.0", "method": "session/cancel", "params": ["sessionId": .string(id)]])
    }

    /// Changes a setting, such as `mode` or `model`.
    public func setConfigOption(_ configId: String, value: String) async throws {
        try await client?.call("session/set_config_option",
            ["sessionId": .string(id), "configId": .string(configId), "type": "id", "value": .string(value)])
    }

    /// Returns when `done()` is true, checking after every change.
    public func until(_ done: @escaping () -> Bool) async {
        if done() { return }
        await withCheckedContinuation { waiters.append((done, $0)) }
    }

    func attach() async throws {
        guard let client else { return }
        var params: JSON = ["sessionId": .string(id)]
        if mirror.seq > 0 { params.set("afterSeq", to: .number(Double(mirror.seq))) }
        replayEnded = false
        let attached = try await client.call(Bridge.sessionAttach, params)
        if attached["catchUp"]?.string == "replay" {
            await until { self.replayEnded }
        } else {
            let head = attached["headSeq"]?.number.map(Int.init) ?? 0
            await until { self.mirror.seq >= head }
        }
    }

    func reset() {
        mirror.reset()
    }

    func sent(_ message: JSON) {
        try? mirror.sent(message)
    }

    func received(_ message: JSON) {
        try? mirror.received(message)
        if message.at("params.update.sessionUpdate")?.string == "replay_complete" { replayEnded = true }
        let ready = waiters.filter { $0.done() }
        waiters.removeAll { $0.done() }
        for waiter in ready { waiter.continuation.resume() }
    }
}
