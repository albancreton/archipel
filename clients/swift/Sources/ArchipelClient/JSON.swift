import Foundation

/// Any JSON value.
public enum JSON: Equatable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSON])
    case object([String: JSON])

    /// The field `key` of an object.
    public subscript(key: String) -> JSON? {
        if case .object(let fields) = self { return fields[key] }
        return nil
    }

    /// The value at a dotted path such as `subject.toolCall.toolCallId`.
    public func at(_ path: String) -> JSON? {
        var current = self
        for key in path.split(separator: ".") {
            guard let next = current[String(key)] else { return nil }
            current = next
        }
        return current
    }

    public var string: String? {
        if case .string(let text) = self { return text }
        return nil
    }

    public var number: Double? {
        if case .number(let value) = self { return value }
        return nil
    }

    public var array: [JSON]? {
        if case .array(let items) = self { return items }
        return nil
    }

    public var object: [String: JSON]? {
        if case .object(let fields) = self { return fields }
        return nil
    }

    public var isNull: Bool { self == .null }

    /// An id as a string, whether it is sent as a string or a number.
    public var idString: String? {
        switch self {
        case .string(let text): return text
        case .number(let value):
            return value == value.rounded() && abs(value) < 1e15 ? String(Int64(value)) : String(value)
        default: return nil
        }
    }

    /// Sets the value at a dotted path, creating objects along the way.
    mutating func set(_ path: String, to value: JSON) {
        var keys = path.split(separator: ".").map(String.init)
        guard !keys.isEmpty else { return }
        let first = keys.removeFirst()
        var fields = object ?? [:]
        if keys.isEmpty {
            fields[first] = value
        } else {
            var child = fields[first] ?? .object([:])
            if child.object == nil { child = .object([:]) }
            child.set(keys.joined(separator: "."), to: value)
            fields[first] = child
        }
        self = .object(fields)
    }
}

extension JSON: Codable {
    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([JSON].self) {
            self = .array(value)
        } else {
            self = .object(try container.decode([String: JSON].self))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let value): try container.encode(value)
        case .number(let value):
            // Whole numbers are written without a fraction, as ids and sequence numbers are.
            if value == value.rounded() && abs(value) < 1e15 {
                try container.encode(Int64(value))
            } else {
                try container.encode(value)
            }
        case .string(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        }
    }

    /// Reads JSON text.
    public init(parsing text: String) throws {
        self = try JSONDecoder().decode(JSON.self, from: Data(text.utf8))
    }

    /// The value as JSON text on one line.
    public var text: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        return (try? encoder.encode(self)).flatMap { String(data: $0, encoding: .utf8) } ?? "null"
    }
}

// No nil literal on purpose: with one, `dictionary[key] = condition ? nil : value` would store
// a JSON null instead of removing the key.
extension JSON: ExpressibleByStringLiteral, ExpressibleByDictionaryLiteral, ExpressibleByArrayLiteral,
    ExpressibleByIntegerLiteral, ExpressibleByBooleanLiteral
{
    public init(stringLiteral value: String) { self = .string(value) }
    public init(dictionaryLiteral elements: (String, JSON)...) { self = .object(Dictionary(elements, uniquingKeysWith: { $1 })) }
    public init(arrayLiteral elements: JSON...) { self = .array(elements) }
    public init(integerLiteral value: Int) { self = .number(Double(value)) }
    public init(booleanLiteral value: Bool) { self = .bool(value) }
}
