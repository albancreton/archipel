import Foundation

// Client state, built by applying ACP v2 messages according to the schema's state rules.
//
// This is a port of `crates/bridge-core/src/state.rs`. Both must give the same state for the
// same messages; `testdata/states` holds what that state is for every recorded session.

/// Points at one entry of a collection.
public struct ItemRef: Hashable, Sendable {
    public var target: String
    public var id: String
    public init(target: String, id: String) {
        self.target = target
        self.id = id
    }
}

/// A request from the agent that waits for the user: an approval or a question.
public struct Request: Equatable, Sendable {
    public var method: String
    public var params: JSON
    /// The answer, once given.
    public var outcome: JSON?
}

public struct ApplyError: Error, CustomStringConvertible {
    public var description: String
}

/// Everything a client holds for one session.
public struct SessionState: Equatable, Sendable {
    /// Target name used for approvals and questions in the timeline.
    public static let requestTarget = "request"
    /// Fields whose streamed text is base64, as `target.field`.
    static let base64Fields: Set<String> = ["terminal.output.data"]

    /// Entries by target, then by id.
    public var collections: [String: [String: JSON]] = [:]
    /// Single values by target: `state`, `usage`, `configOptions`, ...
    public var values: [String: JSON] = [:]
    /// Entries in the order they first appeared.
    public var timeline: [ItemRef] = []
    /// Approvals and questions, by request id.
    public var requests: [String: Request] = [:]
    /// Messages without state, in order: notices and the like.
    public var events: [JSON] = []
    /// Messages whose kind the rules do not know and that carry no rule of their own.
    public var unknown: [JSON] = []

    public init() {}

    /// Applies the body of one `session/update` notification.
    public mutating func applyUpdate(_ update: JSON, rules: RuleSet = .pinned) throws {
        guard let kind = update["sessionUpdate"]?.string else {
            throw ApplyError(description: "update has no `sessionUpdate` field")
        }
        guard let known = rules.updates[kind] else {
            // Not an ACP kind. An extension's update can carry its own rule.
            if let own = Self.ownRule(kind: kind, update: update) {
                try apply(rule: own, kind: kind, body: update)
            } else {
                unknown.append(update)
            }
            return
        }
        guard let session = known.session, !session.isEmpty else {
            events.append(update)
            return
        }
        for rule in session { try apply(rule: rule, kind: kind, body: update) }
    }

    /// Records a request from the agent and applies its state rules.
    public mutating func applyRequest(id: String, method: String, params: JSON, rules: RuleSet = .pinned) throws {
        for rule in rules.requests[method]?.session ?? [] { try apply(rule: rule, kind: method, body: params) }
        let isNew = requests[id] == nil
        requests[id] = Request(method: method, params: params, outcome: nil)
        if isNew { timeline.append(ItemRef(target: Self.requestTarget, id: id)) }
    }

    /// Records the answer to a request. Returns false when the request is not known.
    @discardableResult
    public mutating func resolveRequest(id: String, outcome: JSON) -> Bool {
        guard requests[id] != nil else { return false }
        requests[id]?.outcome = outcome
        return true
    }

    /// Applies the state rules of a response, such as the config options in `session/new`.
    public mutating func applyResponse(method: String, result: JSON, rules: RuleSet = .pinned) throws {
        for rule in rules.responses[method]?.session ?? [] { try apply(rule: rule, kind: method, body: result) }
    }

    /// The entry a timeline reference points at.
    public func item(_ ref: ItemRef) -> JSON? {
        collections[ref.target]?[ref.id]
    }

    /// The requests that have no answer yet, oldest first.
    public var pendingRequests: [(id: String, request: Request)] {
        timeline.compactMap { ref in
            guard ref.target == Self.requestTarget, let request = requests[ref.id], request.outcome == nil else { return nil }
            return (ref.id, request)
        }
    }

    private mutating func apply(rule: StateRule, kind: String, body: JSON) throws {
        var id: String?
        if let path = rule.id {
            guard let found = body.at(path)?.idString else {
                throw ApplyError(description: "`\(kind)` has no id at `\(path)`")
            }
            id = found
        }
        let value: JSON
        if let path = rule.value {
            let found = body.at(path)
            if found == nil || found == .null {
                // Optional data that is absent or null leaves the state alone.
                if rule.action != .append { return }
                if found == nil { throw ApplyError(description: "`\(kind)` has no value at `\(path)`") }
            }
            value = found ?? .null
        } else {
            value = Self.withoutKind(body)
        }

        if rule.action == .remove {
            if let id {
                collections[rule.target]?.removeValue(forKey: id)
                timeline.removeAll { $0.target == rule.target && $0.id == id }
            } else {
                values.removeValue(forKey: rule.target)
            }
            return
        }
        if let id {
            var entries = collections[rule.target] ?? [:]
            if entries[id] == nil {
                let idKey = rule.id?.split(separator: ".").last.map(String.init) ?? "id"
                entries[id] = .object([idKey: .string(id)])
                timeline.append(ItemRef(target: rule.target, id: id))
            }
            entries[id] = Self.change(entries[id] ?? .null, rule: rule, value: value)
            collections[rule.target] = entries
        } else {
            values[rule.target] = Self.change(values[rule.target] ?? .null, rule: rule, value: value)
        }
    }

    /// The state rule an update carries for itself under `_meta.bridge.state`, if any.
    private static func ownRule(kind: String, update: JSON) -> StateRule? {
        guard let rule = update.at("_meta.bridge.state"), rule.object != nil,
              let action = rule["action"]?.string.flatMap(Action.init(rawValue:))
        else { return nil }
        return StateRule(action: action, target: kind, id: rule["id"]?.string, field: rule["field"]?.string, value: rule["value"]?.string)
    }

    /// The message body without its kind tag.
    private static func withoutKind(_ body: JSON) -> JSON {
        guard var fields = body.object else { return body }
        fields.removeValue(forKey: "sessionUpdate")
        return .object(fields)
    }

    private static func change(_ entry: JSON, rule: StateRule, value: JSON) -> JSON {
        if rule.action == .upsert { return merge(entry, value) }
        guard let field = rule.field, rule.action != .remove else { return value }
        var object: JSON = entry.object == nil ? .object([:]) : entry
        if rule.action == .append {
            let isBase64 = base64Fields.contains("\(rule.target).\(field)")
            object.set(field, to: append(object.at(field) ?? .null, value, isBase64: isBase64))
        } else {
            object.set(field, to: value)
        }
        return object
    }

    /// Merges the fields of `incoming` into `entry`. A null field clears that field.
    private static func merge(_ entry: JSON, _ incoming: JSON) -> JSON {
        guard var fields = entry.object, let added = incoming.object else { return incoming }
        for (key, value) in added {
            if value.isNull {
                fields.removeValue(forKey: key)
            } else {
                fields[key] = value
            }
        }
        return .object(fields)
    }

    /// Appends streamed data to a field: text is concatenated (base64 text is decoded first),
    /// a content block is added to a list, and two neighbouring text blocks become one.
    private static func append(_ slot: JSON, _ value: JSON, isBase64: Bool) -> JSON {
        if case .string(let text) = value {
            if slot.isNull { return value }
            if case .string(let existing) = slot { return .string(concat(existing, text, isBase64: isBase64)) }
        }
        if slot.isNull { return .array([value]) }
        if case .array(var blocks) = slot {
            if let last = blocks.last, last["type"]?.string == "text", let existing = last["text"]?.string,
               value["type"]?.string == "text", let more = value["text"]?.string
            {
                var joined = last
                joined.set("text", to: .string(existing + more))
                blocks[blocks.count - 1] = joined
            } else {
                blocks.append(value)
            }
            return .array(blocks)
        }
        return .array([slot, value])
    }

    private static func concat(_ existing: String, _ text: String, isBase64: Bool) -> String {
        if isBase64, let left = Data(base64Encoded: existing), let right = Data(base64Encoded: text) {
            return (left + right).base64EncodedString()
        }
        return existing + text
    }

    /// The state as plain JSON, in the shape of `testdata/states`.
    public var json: JSON {
        .object([
            "collections": .object(collections.mapValues { .object($0) }),
            "values": .object(values),
            "timeline": .array(timeline.map { ["target": .string($0.target), "id": .string($0.id)] }),
            "requests": .object(requests.mapValues { ["method": .string($0.method), "params": $0.params, "outcome": $0.outcome ?? .null] }),
            "events": .array(events),
            "unknown": .array(unknown),
        ])
    }

    /// Rebuilds a state from its JSON form, for example to draw a stored session.
    public init(json: JSON) {
        collections = (json["collections"]?.object ?? [:]).mapValues { $0.object ?? [:] }
        values = json["values"]?.object ?? [:]
        timeline = (json["timeline"]?.array ?? []).compactMap { ref in
            guard let target = ref["target"]?.string, let id = ref["id"]?.string else { return nil }
            return ItemRef(target: target, id: id)
        }
        requests = (json["requests"]?.object ?? [:]).compactMapValues { request in
            guard let method = request["method"]?.string else { return nil }
            let outcome = request["outcome"]
            return Request(method: method, params: request["params"] ?? .null, outcome: outcome == .null ? nil : outcome)
        }
        events = json["events"]?.array ?? []
        unknown = json["unknown"]?.array ?? []
    }
}

/// Which way a message travelled, seen from the client: `in` is from the agent.
public enum Direction: String, Sendable {
    case `in`, out
}

/// One client's view of a conversation: the user's session and its subagents' child sessions.
public struct Conversation: Equatable, Sendable {
    /// Sessions by id.
    public var sessions: [String: SessionState] = [:]
    /// Session ids in the order they first appeared. The first is the one the user talks to.
    public var order: [String] = []
    /// Requests this side sent and has no answer to yet.
    private var outgoing: [String: (method: String, sessionId: String?)] = [:]
    /// Requests from the agent, to find their session when the answer goes out.
    private var incoming: [String: String] = [:]

    public init() {}

    public static func == (left: Conversation, right: Conversation) -> Bool {
        left.sessions == right.sessions && left.order == right.order
    }

    /// The id of the first session seen, which is the one the user talks to.
    public var rootId: String? { order.first }
    public var root: SessionState? { rootId.flatMap { sessions[$0] } }

    /// Changes the state of one session, creating it empty if it is new.
    public mutating func update<T>(_ sessionId: String, _ change: (inout SessionState) throws -> T) rethrows -> T {
        if sessions[sessionId] == nil {
            sessions[sessionId] = SessionState()
            order.append(sessionId)
        }
        return try change(&sessions[sessionId]!)
    }

    /// Applies one JSON-RPC message.
    public mutating func applyMessage(_ direction: Direction, _ message: JSON, rules: RuleSet = .pinned) throws {
        let method = message["method"]?.string
        let id = message["id"]?.idString
        let params = message["params"] ?? .null
        let sessionId = params["sessionId"]?.string

        switch (direction, method, id) {
        case (.in, "session/update"?, nil):
            guard let sessionId, let update = params["update"] else { return }
            try self.update(sessionId) { try $0.applyUpdate(update, rules: rules) }
        case (.in, let method?, let id?):
            guard let sessionId else { return }
            incoming[id] = sessionId
            try self.update(sessionId) { try $0.applyRequest(id: id, method: method, params: params, rules: rules) }
        case (.out, let method?, let id?):
            outgoing[id] = (method, sessionId)
        case (.out, nil, let id?):
            guard let session = incoming.removeValue(forKey: id) else { return }
            let outcome = message["result"] ?? message["error"] ?? .null
            self.update(session) { _ = $0.resolveRequest(id: id, outcome: outcome) }
        case (.in, nil, let id?):
            guard let sent = outgoing.removeValue(forKey: id), let result = message["result"] else { return }
            guard let session = sent.sessionId ?? result["sessionId"]?.string else { return }
            try self.update(session) { try $0.applyResponse(method: sent.method, result: result, rules: rules) }
        default:
            return
        }
    }

    /// The conversation as plain JSON, in the shape of `testdata/states`.
    public var json: JSON {
        ["sessions": .object(sessions.mapValues(\.json))]
    }

    /// Rebuilds a conversation from its JSON text. `order` lists the session ids, root first;
    /// JSON objects do not keep an order by themselves.
    public init(json: JSON, order: [String]) {
        sessions = (json["sessions"]?.object ?? [:]).mapValues(SessionState.init(json:))
        self.order = order.filter { sessions[$0] != nil }
    }
}
