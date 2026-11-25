import Foundation
import Observation

/// The daemon or the agent refused a request.
public struct RPCError: Error, CustomStringConvertible, Equatable {
    public var code: Int
    public var message: String
    /// What the error says beyond its message. For -32004: who has the session open.
    public var data: JSON = .null
    public var description: String { "\(message) (\(code))" }
    /// The session is open in another process of the harness. Fork it to continue.
    public var openElsewhere: Bool { code == -32004 }
    /// The daemon could not write its session log, for example because the disk is full. A
    /// prompt refused with it did not reach the agent. The message says what the storage reported.
    public var storage: Bool { code == -32005 }
    /// What the daemon answers about a session it does not know (JSON-RPC "invalid params").
    static let unknownSession = -32602
}

/// Another process of the harness that has a session open, as the daemon reports it.
public struct OpenElsewhere: Equatable, Sendable {
    public var pid: Int
    /// What that process says it is doing, such as `busy` or `idle`.
    public var status: String?

    public init(pid: Int, status: String? = nil) {
        self.pid = pid
        self.status = status
    }

    init?(_ json: JSON?) {
        guard let json, json != .null else { return nil }
        pid = json["pid"]?.number.map(Int.init) ?? 0
        status = json["status"]?.string
    }
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
    /// Another process of the harness has the session open, such as its own terminal UI. The
    /// daemon then refuses to act on it (error -32004); `fork` makes a copy to use.
    public var openElsewhere: Bool
    /// `running`, `requires_action` or `idle`.
    public var state: String?
    public var headSeq: Int
    public var updatedAt: String?

    /// From the daemon's JSON, as in `_bridge/sessions/list`.
    public init?(_ json: JSON) {
        guard let id = json["sessionId"]?.string else { return nil }
        self.id = id
        agentId = json["agentId"]?.string ?? ""
        title = json["title"]?.string
        cwd = json["cwd"]?.string ?? ""
        live = json["live"] == .bool(true)
        logged = json["logged"] != .bool(false)
        openElsewhere = json["openElsewhere"] == .bool(true)
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
                // Every session continues from the last sequence number it applied. One the
                // daemon no longer knows was deleted while the connection was down.
                for session in Array(followed.values) {
                    do {
                        try await session.attach()
                    } catch let error as RPCError where error.code == RPCError.unknownSession {
                        removed(session.id)
                    }
                }
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
                    code: error["code"]?.number.map(Int.init) ?? 0, message: error["message"]?.string ?? "refused",
                    data: error["data"] ?? .null))
            } else {
                continuation?.resume(returning: message["result"] ?? .null)
            }
            return
        }
        if message["method"]?.string == Bridge.sessionsRemoved {
            if let id = message.at("params.sessionId")?.string { removed(id) }
            return
        }
        if message["method"]?.string == Bridge.sessionError {
            if let id = message.at("params.sessionId")?.string, let code = message.at("params.code")?.number,
               let text = message.at("params.message")?.string {
                followed[id]?.failed(RPCError(code: Int(code), message: text))
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

    /// Reads at most `limit` sessions, newest first, without touching `sessions`. `nextCursor`
    /// is there when more follow: pass it as `cursor` to get them. A session that changes
    /// between two pages moves to the front, where `sessions` already has it.
    public func sessionsPage(limit: Int, cursor: String? = nil, loggedOnly: Bool = false) async throws
        -> (sessions: [SessionSummary], nextCursor: String?) {
        var params: JSON = ["limit": .number(Double(limit)), "loggedOnly": .bool(loggedOnly)]
        if let cursor { params.set("cursor", to: .string(cursor)) }
        let result = try await call(Bridge.sessionsList, params)
        return ((result["sessions"]?.array ?? []).compactMap(SessionSummary.init), result["nextCursor"]?.string)
    }

    /// Creates a session and attaches to it.
    ///
    /// `configOptions` are settings to start with, by id: a value of a choice (`.string`) or a
    /// switch (`.bool`). Only for an agent whose capabilities have `session.options`. One the
    /// agent does not know is ignored, not refused.
    public func newSession(cwd: String, agentId: String? = nil, configOptions: [String: JSON] = [:]) async throws -> BridgeSession {
        var params: JSON = ["cwd": .string(cwd), "mcpServers": []]
        if !configOptions.isEmpty {
            params.set("configOptions", to: .array(configOptions.sorted { $0.key < $1.key }.map { id, value in
                let isSwitch = if case .bool = value { true } else { false }
                return ["configId": .string(id), "type": isSwitch ? "boolean" : "id", "value": value]
            }))
        }
        if let agentId { params.set("_meta.bridge.agentId", to: .string(agentId)) }
        let created = try await call("session/new", params)
        guard let id = created["sessionId"]?.string else { throw RPCError(code: 0, message: "the agent did not return a session id") }
        return try await session(id)
    }

    /// Makes a copy of a session to continue with and attaches to it. The original is left as
    /// it is. This is the way on when a session is open somewhere else.
    public func fork(_ id: String) async throws -> BridgeSession {
        let forked = try await call("session/fork", ["sessionId": .string(id)])
        guard let copy = forked["sessionId"]?.string else { throw RPCError(code: 0, message: "the agent did not return a session id") }
        return try await session(copy)
    }

    /// The agents the daemon can start. `capabilities` is what an agent said it can do, and is
    /// there once the agent has been started.
    public func agents() async throws -> [JSON] {
        try await call(Bridge.agentsList)["agents"]?.array ?? []
    }

    /// The settings a session would start with (`configOptions`) and its commands, without
    /// making one: for a settings bar shown before the first message. With `sessionId`, those
    /// of an existing session, which is not woken for it. Only where the agent's capabilities
    /// have `session.options`; otherwise the call fails with code -32601.
    public func sessionOptions(cwd: String, agentId: String? = nil, sessionId: String? = nil) async throws -> JSON {
        var params: JSON = ["cwd": .string(cwd)]
        if let sessionId { params.set("sessionId", to: .string(sessionId)) }
        if let agentId { params.set("_meta.bridge.agentId", to: .string(agentId)) }
        return try await call("session/options", params)
    }

    /// Renames a session and returns the title the harness stored, which may differ from the
    /// one sent. Every app following the session or the list hears of it. Only where the
    /// agent's capabilities have `session.title`.
    @discardableResult
    public func setTitle(_ id: String, to title: String) async throws -> String {
        let result = try await call("session/set_title", ["sessionId": .string(id), "title": .string(title)])
        return result["title"]?.string ?? title
    }

    /// Deletes a session at its harness and in the daemon. It cannot be undone, and needs the
    /// `admin` scope (the local socket has it). Only where the agent's capabilities have
    /// `session.delete`.
    public func deleteSession(_ id: String) async throws {
        _ = try await call("session/delete", ["sessionId": .string(id)])
        // The daemon tells every app, this one included; this covers a caller that goes on at once.
        removed(id)
    }

    /// A session was deleted, by this app or another: it leaves the list and is no longer followed.
    private func removed(_ id: String) {
        sessions.removeAll { $0.id == id }
        followed.removeValue(forKey: id)?.markRemoved()
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
    /// Who else has the session open, as far as the daemon last said: another process of the
    /// harness, such as its own terminal UI. While it does, the daemon refuses prompts and
    /// setting changes (error -32004); `fork()` makes a copy to continue with. Known when the
    /// session is attached and after every prompt, so it can be out of date in between.
    public private(set) var openElsewhere: OpenElsewhere?
    /// The session was deleted, by this app or another. What it held stays readable; it gets
    /// no more changes, and acting on it fails.
    public private(set) var removed = false
    /// Something went wrong in the session outside any request, as far as the daemon last said.
    /// With `error.storage`: the daemon cannot write its log, so what happens now is not kept
    /// and not shown. It is cleared by the next change that arrives.
    public private(set) var error: RPCError?

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
        try await act("session/prompt", ["sessionId": .string(id), "prompt": [["type": "text", "text": .string(text)]]])
    }

    /// Makes a copy of this session to continue with, and attaches to it. This one is left as it is.
    public func fork() async throws -> BridgeSession {
        guard let client else { throw ConnectionClosed() }
        return try await client.fork(id)
    }

    /// Sends a request that acts on the session, and notes whether another process stood in the way.
    private func act(_ method: String, _ params: JSON) async throws {
        do {
            try await client?.call(method, params)
        } catch let error as RPCError where error.openElsewhere {
            openElsewhere = OpenElsewhere(error.data) ?? OpenElsewhere(pid: 0)
            throw error
        }
        if openElsewhere != nil { openElsewhere = nil }
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

    /// Changes a setting to one of its values, such as `mode` to `plan`.
    public func setConfigOption(_ configId: String, value: String) async throws {
        try await act("session/set_config_option",
            ["sessionId": .string(id), "configId": .string(configId), "type": "id", "value": .string(value)])
    }

    /// Switches an on/off setting.
    public func setConfigOption(_ configId: String, on: Bool) async throws {
        try await act("session/set_config_option",
            ["sessionId": .string(id), "configId": .string(configId), "type": "boolean", "value": .bool(on)])
    }

    /// Returns when `done()` is true, checking after every change. Also returns when the
    /// session is deleted, since nothing will change after that: check `removed`.
    public func until(_ done: @escaping () -> Bool) async {
        if done() || removed { return }
        await withCheckedContinuation { waiters.append((done, $0)) }
    }

    func attach() async throws {
        guard let client else { return }
        var params: JSON = ["sessionId": .string(id)]
        if mirror.seq > 0 { params.set("afterSeq", to: .number(Double(mirror.seq))) }
        replayEnded = false
        let attached = try await client.call(Bridge.sessionAttach, params)
        openElsewhere = OpenElsewhere(attached["openElsewhere"])
        if attached["catchUp"]?.string == "replay" {
            await until { self.replayEnded }
        } else {
            let head = attached["headSeq"]?.number.map(Int.init) ?? 0
            await until { self.mirror.seq >= head }
        }
    }

    func failed(_ error: RPCError) {
        self.error = error
        wake()
    }

    /// Lets the waits whose condition now holds go on.
    private func wake() {
        let ready = waiters.filter { $0.done() }
        waiters.removeAll { $0.done() }
        for waiter in ready { waiter.continuation.resume() }
    }

    func markRemoved() {
        removed = true
        let waiting = waiters
        waiters.removeAll()
        for waiter in waiting { waiter.continuation.resume() }
    }

    func reset() {
        mirror.reset()
    }

    func sent(_ message: JSON) {
        try? mirror.sent(message)
    }

    func received(_ message: JSON) {
        let before = mirror.seq
        try? mirror.received(message)
        if mirror.seq > before, error != nil { error = nil }
        if message.at("params.update.sessionUpdate")?.string == "replay_complete" { replayEnded = true }
        wake()
    }
}
