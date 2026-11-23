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

    func testTextOfContent() {
        XCTAssertEqual(contentText([["type": "text", "text": "a"], ["type": "text", "text": "b"]]), "ab")
        XCTAssertEqual(contentText(["type": "content", "content": ["type": "text", "text": "inner"]]), "inner")
        XCTAssertEqual(contentText(["type": "image"]), "[image]")
    }
}
