import ArchipelClient
import ArchipelMinitel
import ArchipelUI
import SwiftUI
import XCTest

/// Draws recorded sessions and the parts around them with the Minitel theme, dark and light, off
/// screen, into `.build/archipel-ui-renders/minitel`, to look at; and checks what the theme promises.
@MainActor
final class MinitelTests: XCTestCase {
    static let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    static let out = root.appendingPathComponent(".build/archipel-ui-renders/minitel")

    /// A recorded session, rebuilt by applying its messages; with `waiting`, its requests are open again.
    func conversation(_ name: String, waiting: Bool = false) throws -> Conversation {
        var conversation = Conversation()
        let text = try String(contentsOf: Self.root.appendingPathComponent("testdata/transcripts/\(name).ndjson"), encoding: .utf8)
        for line in text.split(separator: "\n") {
            let frame = try JSON(parsing: String(line))
            if let message = frame["m"], let direction = frame["dir"]?.string.flatMap(Direction.init(rawValue:)) {
                try conversation.applyMessage(direction, message)
            }
        }
        if waiting, let id = conversation.rootId {
            for key in conversation.sessions[id]!.requests.keys { conversation.sessions[id]!.requests[key]!.outcome = nil }
        }
        return conversation
    }

    /// Draws a view in both schemes and returns the sizes of the two pictures.
    @discardableResult
    func draw(_ view: some View, as name: String, width: CGFloat = 760) throws -> [Int] {
        try FileManager.default.createDirectory(at: Self.out, withIntermediateDirectories: true)
        return try [ColorScheme.dark, .light].map { scheme in
            let theme = ArchipelTheme.minitel(scheme)
            let framed = view.padding(20).frame(width: width).background(theme.colors.background ?? .clear).archipelTheme(theme)
            let renderer = ImageRenderer(content: framed)
            renderer.scale = 2
            let tiff = try XCTUnwrap(renderer.nsImage?.tiffRepresentation, name)
            let png = try XCTUnwrap(NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]))
            try png.write(to: Self.out.appendingPathComponent("\(name)-\(scheme == .dark ? "dark" : "light").png"))
            return png.count
        }
    }

    func testRecordedSessionsAreDrawn() throws {
        for name in ["claude-v2/files", "claude-v2/subagent", "claude-v2/plan", "claude-v2/command_output", "claude-v2/thinking"] {
            let conversation = try conversation(name)
            let state = try XCTUnwrap(conversation.root, name)
            let view = VStack(alignment: .leading, spacing: 14) {
                StatusBar(state: state)
                SessionTimeline(conversation: conversation, state: state)
            }
            for size in try draw(view, as: name.replacingOccurrences(of: "/", with: "-")) {
                XCTAssertGreaterThan(size, 20_000, "\(name): something was drawn")
            }
        }
    }

    func testWhatWaitsOnTheUserIsDrawn() throws {
        let approval = try conversation("claude-v2/approval_command", waiting: true)
        let question = try conversation("claude-v2/question", waiting: true)
        let requests: [Request] = [approval, question].compactMap { $0.root }.flatMap { Array($0.requests.values) }
        XCTAssertEqual(requests.count, 2)
        let view = VStack(alignment: .leading, spacing: 14) {
            ForEach(Array(requests.enumerated()), id: \.offset) { _, request in
                if request.method == "session/request_permission" { ApprovalView(request: request) } else { QuestionView(request: request) }
            }
        }
        try draw(view, as: "waiting")
    }

    func testTheComposerSettingsAndSessionListAreDrawn() throws {
        let state = try XCTUnwrap(conversation("claude-v2/simple").root)
        let now = try XCTUnwrap(ISO8601DateFormatter().date(from: "2026-11-24T12:00:00Z"))
        let sessions: [SessionSummary] = [
            ["sessionId": "s1", "agentId": "claude", "cwd": "/work/app", "title": "Fix the login", "state": "requires_action", "updatedAt": "2026-11-24T11:55:00Z"],
            ["sessionId": "s2", "agentId": "claude", "cwd": "/work/site", "title": "Docs", "state": "running", "openElsewhere": true, "updatedAt": "2026-11-24T08:59:59Z"],
            ["sessionId": "s3", "agentId": "pi", "cwd": "/work/app", "logged": false],
        ].compactMap { SessionSummary($0) }
        let view = VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 8) { ForEach(sessions) { SessionRow(session: $0, now: now) } }.frame(width: 300)
            AgentStatus(agents: [["agentId": "pi", "name": "Pi", "readiness": "needs_login"]]) {}
            OpenElsewhereNotice(holder: OpenElsewhere(pid: 4242, status: "busy")) {}
            SettingsBar(settings: Setting.all(from: state.values["configOptions"])) { _, _ in }
            Composer(running: true, send: { _ in }, commands: [], accepts: Accepts())
        }
        try draw(view, as: "around")
    }

    /// What the theme promises: dark by default on #282C34, nothing under 14 points, amber only
    /// for what waits on the user.
    func testTheThemeKeepsItsPromises() {
        let dark = ArchipelTheme.minitel()
        XCTAssertEqual(dark.colorScheme, .dark)
        XCTAssertEqual(ArchipelTheme.minitel(.light).colorScheme, .light)
        XCTAssertEqual(dark.colors.ansi.count, 16)
        XCTAssertTrue(dark.drawsControls)
        XCTAssertEqual(dark.badge, .tinted)
    }
}
