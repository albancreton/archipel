import BridgeClient
import SwiftUI
import XCTest
@testable import BridgeUI

/// Draws recorded sessions with the SwiftUI views, off screen, and keeps the pictures.
@MainActor
final class RenderTests: XCTestCase {
    static let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()

    /// A recorded session, rebuilt by applying its messages.
    func conversation(_ name: String) throws -> Conversation {
        var conversation = Conversation()
        let text = try String(contentsOf: Self.root.appendingPathComponent("testdata/transcripts/\(name).ndjson"), encoding: .utf8)
        for line in text.split(separator: "\n") {
            let frame = try JSON(parsing: String(line))
            if let message = frame["m"], let direction = frame["dir"]?.string.flatMap(Direction.init(rawValue:)) {
                try conversation.applyMessage(direction, message)
            }
        }
        return conversation
    }

    func testRecordedSessionsAreDrawn() throws {
        let out = Self.root.appendingPathComponent(".build/bridge-ui-renders")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        for name in ["claude-v2/files", "claude-v2/subagent", "claude-v2/approval_denied", "claude-v2/question", "claude-v2/plan", "pi-v2/files"] {
            let conversation = try conversation(name)
            let state = try XCTUnwrap(conversation.root, name)
            let view = VStack(alignment: .leading, spacing: 12) {
                StatusBar(state: state)
                Divider()
                SessionTimeline(conversation: conversation, state: state)
            }
            .padding(16)
            .frame(width: 720)
            .background(Color(nsColor: .windowBackgroundColor))
            .environment(\.colorScheme, .light)
            let renderer = ImageRenderer(content: view)
            renderer.scale = 2
            let image = try XCTUnwrap(renderer.nsImage, name)
            XCTAssertGreaterThan(image.size.height, 150, "\(name): something was drawn")
            let tiff = try XCTUnwrap(image.tiffRepresentation)
            let png = try XCTUnwrap(NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]))
            try png.write(to: out.appendingPathComponent(name.replacingOccurrences(of: "/", with: "-") + ".png"))
        }
    }

    /// The notice a chat shows when another process has the session open, with the fork button.
    func testASessionOpenElsewhereIsDrawnWithTheFork() throws {
        let conversation = try conversation("claude-v2/simple")
        var actions = ChatActions()
        actions.prompt = { _ in }
        actions.fork = {}
        let view = ChatView(conversation: conversation, actions: actions, openElsewhere: OpenElsewhere(pid: 4242, status: "busy"))
            .frame(width: 720, height: 420)
            .background(Color(nsColor: .windowBackgroundColor))
            .environment(\.colorScheme, .light)
        let renderer = ImageRenderer(content: view)
        renderer.scale = 2
        let tiff = try XCTUnwrap(renderer.nsImage?.tiffRepresentation)
        let png = try XCTUnwrap(NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]))
        let out = Self.root.appendingPathComponent(".build/bridge-ui-renders")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        try png.write(to: out.appendingPathComponent("open-elsewhere.png"))
        XCTAssertGreaterThan(png.count, 10_000, "something was drawn")
    }

    /// Draws a view off screen into `.build/bridge-ui-renders/<name>.png` and returns its size in bytes.
    func draw(_ view: some View, as name: String) throws -> Int {
        let renderer = ImageRenderer(content: view.background(Color(nsColor: .windowBackgroundColor)).environment(\.colorScheme, .light))
        renderer.scale = 2
        let tiff = try XCTUnwrap(renderer.nsImage?.tiffRepresentation)
        let png = try XCTUnwrap(NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]))
        let out = Self.root.appendingPathComponent(".build/bridge-ui-renders")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        try png.write(to: out.appendingPathComponent(name + ".png"))
        return png.count
    }

    func testSettingsAreReadByKindAndDrawn() throws {
        let claude = try XCTUnwrap(conversation("claude-v2/simple").root)
        let settings = Setting.all(from: claude.values["configOptions"])
        XCTAssertEqual(settings.map(\.id), ["model", "effort", "ultracode", "mode"])
        XCTAssertEqual(settings.map(\.category), ["model", "thought_level", "model_config", "mode"])
        XCTAssertEqual(settings[0].current, .choice("haiku"))
        XCTAssertEqual(settings[2].current, .on(false))
        let pi = try XCTUnwrap(conversation("pi-v2/simple").root)
        XCTAssertEqual(Setting.all(from: pi.values["configOptions"]).map(\.id), ["model", "thought_level"])

        // Values under headings, and a kind this kit does not know.
        let grouped = Setting.all(from: [
            ["configId": "model", "name": "Model", "type": "select", "currentValue": "b", "options": [
                ["groupId": "g1", "name": "Fast", "options": [["value": "a", "name": "A"]]],
                ["groupId": "g2", "name": "Careful", "options": [["value": "b", "name": "B"]]],
            ]],
            ["configId": "budget", "name": "Budget", "type": "number", "currentValue": 3],
        ])
        XCTAssertEqual(grouped.count, 1)
        XCTAssertEqual(grouped[0].groups.map(\.name), ["Fast", "Careful"])
        XCTAssertEqual(Setting.all(from: nil), [])

        let bar = SettingsBar(settings: settings) { _, _ in }.padding(12).frame(width: 720)
        XCTAssertGreaterThan(try draw(bar, as: "settings-bar"), 3000, "something was drawn")
    }

    func testTheSessionListGroupsByFolderAndOffersOnlyWhatTheAgentCanDo() throws {
        let now = try XCTUnwrap(ISO8601DateFormatter().date(from: "2026-11-24T12:00:00Z"))
        let sessions: [SessionSummary] = [
            ["sessionId": "s1", "agentId": "claude", "cwd": "/work/app", "title": "Fix the login", "state": "requires_action", "updatedAt": "2026-11-24T11:55:00Z"],
            ["sessionId": "s2", "agentId": "claude", "cwd": "/work/site", "title": "Docs", "state": "running", "openElsewhere": true, "updatedAt": "2026-11-24T08:59:59.250Z"],
            ["sessionId": "s3", "agentId": "pi", "cwd": "/work/app", "logged": false],
            ["sessionId": "s4", "agentId": "claude", "cwd": "/work/app", "title": "Old", "state": "idle", "updatedAt": "2026-09-01T00:00:00Z"],
        ].compactMap { SessionSummary($0) }
        XCTAssertEqual(SessionSummary.byFolder(sessions).map { "\($0.cwd): \($0.sessions.map(\.id).joined(separator: " "))" },
            ["/work/app: s1 s3 s4", "/work/site: s2"])
        XCTAssertEqual(sessions.map { $0.ago(now: now) }, ["5m", "3h", "", "2026-09-01"])
        XCTAssertEqual(sessions.map { $0.attention?.text }, ["needs you", "running", nil, nil])

        let agents: [JSON] = [
            ["agentId": "claude", "capabilities": ["session": ["title": [:], "delete": [:]]]],
            ["agentId": "pi", "capabilities": ["session": [:]]],
        ]
        let list = SessionList(sessions: sessions, selection: .constant("s1"), agents: agents, rename: { _, _ in }, delete: { _ in })
        XCTAssertTrue(list.can(sessions[0], "title"))
        XCTAssertTrue(list.can(sessions[0], "delete"))
        XCTAssertFalse(list.can(sessions[2], "title"), "Pi does not say it can rename")
        XCTAssertTrue(SessionList(sessions: sessions, selection: .constant(nil)).can(sessions[2], "title"), "without the agent list, everything is offered")

        // A `List` draws nothing off screen, so the rows are drawn the way the list stacks them.
        let rows = VStack(alignment: .leading, spacing: 6) {
            ForEach(sessions) { SessionRow(session: $0, now: now) }
        }
        .padding(12).frame(width: 280)
        XCTAssertGreaterThan(try draw(rows, as: "session-rows"), 5000, "something was drawn")
    }

    func testTextOfContent() {
        XCTAssertEqual(contentText([["type": "text", "text": "a"], ["type": "text", "text": "b"]]), "ab")
        XCTAssertEqual(contentText(["type": "content", "content": ["type": "text", "text": "inner"]]), "inner")
        XCTAssertEqual(contentText(["type": "image"]), "[image]")
    }
}
