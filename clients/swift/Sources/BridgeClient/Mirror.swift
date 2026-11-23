import Foundation

// What a client of the daemon holds for one session, and how the daemon's messages change it.
// A port of `crates/bridge-core/src/mirror.rs`.

public enum Bridge {
    public static let requestResolved = "_bridge/request/resolved"
    public static let sessionsUpdated = "_bridge/sessions/updated"
    public static let sessionsRemoved = "_bridge/sessions/removed"
    public static let sessionAttach = "_bridge/session/attach"
    public static let sessionDetach = "_bridge/session/detach"
    public static let sessionsList = "_bridge/sessions/list"
    public static let agentsList = "_bridge/agents/list"

    /// The log position the daemon put on a message, if any.
    public static func seq(of message: JSON) -> Int? {
        message.at("params._meta.bridge.seq")?.number.map(Int.init)
    }

    /// The session whose log a message belongs to: for a subagent's messages, its parent's.
    public static func root(of message: JSON) -> String? {
        (message.at("params._meta.bridge.rootSessionId") ?? message.at("params.sessionId"))?.string
    }
}

/// A client's copy of one session, including its subagents' child sessions.
public struct Mirror: Equatable, Sendable {
    public var conversation = Conversation()
    /// The newest sequence number applied. Sent as `afterSeq` when attaching again.
    public var seq = 0

    public init() {}

    /// Forgets everything, before a full replay.
    public mutating func reset() {
        self = Mirror()
    }

    /// Notes a request this client sent, so the response can be applied when it arrives.
    public mutating func sent(_ message: JSON, rules: RuleSet = .pinned) throws {
        // An answer to the agent is not applied here: the daemon confirms it with
        // `_bridge/request/resolved`, because another client may have answered first.
        if message["method"] == nil { return }
        try conversation.applyMessage(.out, message, rules: rules)
    }

    /// Applies a message from the daemon.
    public mutating func received(_ message: JSON, rules: RuleSet = .pinned) throws {
        if let seq = Bridge.seq(of: message) { self.seq = max(self.seq, seq) }
        let params = message["params"] ?? .null
        // The marker that ends a replay says nothing about the session itself.
        if params.at("update.sessionUpdate")?.string == "replay_complete" { return }
        let sessionId = params["sessionId"]?.string
        let method = message["method"]?.string

        if method == Bridge.requestResolved, let sessionId {
            guard let id = params["requestId"]?.idString else { return }
            let outcome = params["outcome"] ?? .null
            try conversation.update(sessionId) { session in
                if session.requests[id] == nil, let requestMethod = params["method"]?.string, let request = params["params"] {
                    try session.applyRequest(id: id, method: requestMethod, params: request, rules: rules)
                }
                session.resolveRequest(id: id, outcome: outcome)
            }
        } else if let method, let id = message["id"]?.idString, let sessionId {
            try conversation.update(sessionId) { session in
                // Sent again after a reconnect so it can be answered on this connection.
                if session.requests[id] != nil { return }
                try session.applyRequest(id: id, method: method, params: Self.withoutPosition(params), rules: rules)
            }
        } else {
            try conversation.applyMessage(.in, message, rules: rules)
        }
    }

    /// The request as the agent sent it, without the daemon's position mark.
    private static func withoutPosition(_ params: JSON) -> JSON {
        guard var fields = params.object, var meta = fields["_meta"]?.object else { return params }
        meta.removeValue(forKey: "bridge")
        if meta.isEmpty {
            fields.removeValue(forKey: "_meta")
        } else {
            fields["_meta"] = .object(meta)
        }
        return .object(fields)
    }
}
