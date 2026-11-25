import Foundation
import XCTest
@testable import ArchipelClient

/// The reducer must give exactly the state the Rust core gives, for every recorded session.
final class StateTests: XCTestCase {
    static let testdata = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("testdata")

    func frames(_ url: URL) throws -> [(Direction, JSON)] {
        try String(contentsOf: url, encoding: .utf8).split(separator: "\n").compactMap { line in
            let frame = try JSON(parsing: String(line))
            guard let message = frame["m"], let dir = frame["dir"]?.string.flatMap(Direction.init(rawValue:)) else { return nil }
            return (dir, message)
        }
    }

    func files(_ folder: String, suffix: String) throws -> [(name: String, url: URL)] {
        let root = Self.testdata.appendingPathComponent(folder)
        var found: [(String, URL)] = []
        for set in try FileManager.default.contentsOfDirectory(atPath: root.path).sorted() {
            let directory = root.appendingPathComponent(set)
            for file in try FileManager.default.contentsOfDirectory(atPath: directory.path).sorted() where file.hasSuffix(suffix) {
                found.append(("\(set)/\(file.dropLast(suffix.count))", directory.appendingPathComponent(file)))
            }
        }
        return found
    }

    func testEveryRecordedSessionGivesTheReferenceState() throws {
        let recordings = try files("transcripts", suffix: ".ndjson")
        XCTAssertGreaterThanOrEqual(recordings.count, 29)
        for (name, url) in recordings {
            var conversation = Conversation()
            for (direction, message) in try frames(url) {
                try conversation.applyMessage(direction, message)
            }
            let expected = try JSON(parsing: String(contentsOf: Self.testdata.appendingPathComponent("states/\(name).json"), encoding: .utf8))
            XCTAssertNil(difference(conversation.json, expected), name)
        }
    }

    /// What an app sees through the daemon: live, and as a replay. Both must end in the same state.
    func testDaemonRecordingsGiveTheReferenceState() throws {
        let states = try files("daemon", suffix: ".state.json")
        XCTAssertGreaterThanOrEqual(states.count, 7)
        for (name, url) in states {
            let expected = try JSON(parsing: String(contentsOf: url, encoding: .utf8))
            for kind in ["live", "replay"] {
                var mirror = Mirror()
                for (direction, message) in try frames(Self.testdata.appendingPathComponent("daemon/\(name).\(kind).ndjson")) {
                    if direction == .out {
                        try mirror.sent(message)
                    } else {
                        // An attach that announces a replay starts from nothing.
                        if message.at("result.catchUp")?.string == "replay" { mirror.reset() }
                        try mirror.received(message)
                    }
                }
                XCTAssertNil(difference(mirror.conversation.json, expected), "\(name) \(kind)")
                XCTAssertGreaterThan(mirror.seq, 0)
            }
        }
    }

    func testAStateSurvivesBeingWrittenAndReadBack() throws {
        let url = Self.testdata.appendingPathComponent("states/claude-v2/subagent.json")
        let json = try JSON(parsing: String(contentsOf: url, encoding: .utf8))
        let conversation = Conversation(json: json, order: Array((json["sessions"]?.object ?? [:]).keys))
        XCTAssertEqual(conversation.json, json)
        XCTAssertEqual(try JSON(parsing: json.text), json)
    }
}

/// Where two JSON values first differ, or nil when they are equal. Easier to read than both values.
func difference(_ got: JSON, _ expected: JSON, at path: String = "") -> String? {
    switch (got, expected) {
    case (.object(let left), .object(let right)):
        for key in Set(left.keys).union(right.keys).sorted() {
            guard let a = left[key] else { return "\(path).\(key): missing" }
            guard let b = right[key] else { return "\(path).\(key): unexpected" }
            if let found = difference(a, b, at: "\(path).\(key)") { return found }
        }
        return nil
    case (.array(let left), .array(let right)):
        if left.count != right.count { return "\(path): \(left.count) items, expected \(right.count)" }
        for (index, pair) in zip(left, right).enumerated() {
            if let found = difference(pair.0, pair.1, at: "\(path)[\(index)]") { return found }
        }
        return nil
    default:
        return got == expected ? nil : "\(path): \(got.text.prefix(120)) instead of \(expected.text.prefix(120))"
    }
}
