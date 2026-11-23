import Foundation

// State rules: how each ACP message changes what a client holds.
//
// ACP v2 annotates its schema with `x-state`. The repository extracts those into
// `state-rules.json`; this package carries a copy as a resource. A client that follows the
// rules needs no code per message kind.

public enum Action: String, Codable, Sendable {
    case append, upsert, replace, remove
}

/// One state change. Paths are dotted and relative to the message body.
public struct StateRule: Codable, Sendable, Equatable {
    public var action: Action
    /// The kind of thing changed, such as `agentMessage`, `toolCall` or `state`.
    public var target: String
    /// Where the entry's id is in the message. Absent when the target is a single value.
    public var id: String?
    /// The field of the target that is changed. Absent means the whole entry.
    public var field: String?
    /// Where the new data is in the message. Absent means the whole message body.
    public var value: String?
}

public struct Rules: Codable, Sendable {
    public var session: [StateRule]?
}

public struct RuleSet: Codable, Sendable {
    public var source: String
    public var updates: [String: Rules]
    public var requests: [String: Rules]
    public var responses: [String: Rules]

    /// The rules of the ACP v2 revision this package was built for.
    public static let pinned: RuleSet = {
        guard let url = Bundle.module.url(forResource: "state-rules", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let rules = try? JSONDecoder().decode(RuleSet.self, from: data)
        else { fatalError("state-rules.json is missing from the BridgeClient package") }
        return rules
    }()
}
