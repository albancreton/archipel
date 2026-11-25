import BridgeClient
import SwiftUI
import XCTest
@testable import BridgeUI

/// The SwiftUI views used as a person would: buttons pressed, text typed. See `Screen`.
@MainActor
final class ClickTests: XCTestCase {
    struct Refused: Error, CustomStringConvertible { let description: String }

    /// What a view asked its app to do. A class, so the views' closures can write to it.
    final class Calls: @unchecked Sendable {
        var made: [String] = []
        /// While set, the app says no with this reason.
        var refuse: String?
        func note(_ call: String) throws {
            made.append(call)
            if let refuse { throw Refused(description: refuse) }
        }
    }

    /// A recorded session, with its requests waiting for an answer again.
    func waiting(_ name: String) throws -> Conversation {
        var conversation = try RenderTests().conversation(name)
        let id = try XCTUnwrap(conversation.rootId)
        for key in conversation.sessions[id]!.requests.keys { conversation.sessions[id]!.requests[key]!.outcome = nil }
        return conversation
    }

    func testAnApprovalIsAnsweredWithOnePress() throws {
        let calls = Calls()
        var actions = ChatActions()
        actions.approve = { try? calls.note("\($0):\($1)") }
        let conversation = try waiting("claude-v2/approval_denied")
        let screen = Screen(height: 700) { ChatView(conversation: conversation, actions: actions) }
        XCTAssertEqual([screen.buttons.first, screen.buttons.last], ["Yes", "No"])
        XCTAssertEqual(calls.made, [], "nothing is chosen by drawing")
        screen.press("No")
        XCTAssertEqual(calls.made, ["0:reject"])
    }

    func testAQuestionIsAnsweredWithItsFormOrSkipped() throws {
        let calls = Calls()
        var actions = ChatActions()
        actions.answer = { id, content in try? calls.note("\(id):\(content.map { String(describing: $0) } ?? "skipped")") }
        let conversation = try waiting("claude-v2/question")
        let screen = Screen(height: 700) { ChatView(conversation: conversation, actions: actions) }
        XCTAssertTrue(screen.shows("Which colour do you prefer?"))
        screen.type("teal", into: "Other")
        screen.press("Answer")
        XCTAssertEqual(calls.made.count, 1)
        XCTAssertTrue(calls.made[0].contains("question_0_custom") && calls.made[0].contains("teal"), calls.made[0])
        screen.press("Skip")
        XCTAssertEqual(calls.made.last, "0:skipped")
    }

    func testTheComposerSendsOffersCommandsAndGivesBackARefusedMessage() throws {
        let calls = Calls()
        let field = "Message the agent…"
        let screen = Screen {
            Composer(
                running: false,
                send: { content in try calls.note(content.map { $0["text"]?.string ?? $0["uri"]?.string ?? "?" }.joined(separator: " | ")) },
                commands: [Command(name: "deep-research", description: "Research a topic"), Command(name: "review", description: "Review the changes"), Command(name: "rewind")],
                mentions: { query in
                    [Mention(uri: "file:///work/main.rs", name: "main.rs"), Mention(uri: "file:///work/Makefile", name: "Makefile"), Mention(uri: "file:///work/README.md", name: "README.md")]
                        .filter { $0.name.lowercased().contains(query.lowercased()) }
                })
        }
        // Nothing is sent from an empty field.
        screen.press("Send")
        screen.pressReturn(in: field)
        XCTAssertEqual(calls.made, [])

        screen.type("hello", into: field)
        screen.pressReturn(in: field)
        screen.wait("the message is sent") { calls.made == ["hello"] }
        XCTAssertEqual(screen.text(in: field), "")

        // Commands: names that start with what was typed come first; Return takes the first.
        screen.type("/re", into: field)
        let offered = screen.buttons.filter { $0.hasPrefix("/") }
        XCTAssertEqual(offered.map { $0.split(separator: ",")[0] }, ["/review", "/rewind", "/deep-research"], "\(offered)")
        screen.pressReturn(in: field)
        XCTAssertEqual(screen.text(in: field), "/review ")
        XCTAssertEqual(calls.made.count, 1, "Return took the command and sent nothing")
        screen.type("the diff", into: field)
        screen.press("Send")
        screen.wait("the command is sent") { calls.made.last == "/review the diff" }

        // A press on an entry takes it too.
        screen.type("/re", into: field)
        screen.press(try XCTUnwrap(screen.buttons.first { $0.hasPrefix("/rewind") }))
        XCTAssertEqual(screen.text(in: field), "/rewind ")
        screen.pressReturn(in: field)
        screen.wait("the command is sent") { calls.made.last == "/rewind " }

        // Mentions: the file is named in the text and sent as a link.
        screen.type("look at @ma", into: field)
        screen.wait("the files are offered") { screen.buttons.contains { $0.hasPrefix("@main.rs") } }
        XCTAssertEqual(screen.buttons.filter { $0.hasPrefix("@") }.map { $0.split(separator: ",")[0] }, ["@main.rs", "@Makefile"])
        screen.pressReturn(in: field)
        XCTAssertEqual(screen.text(in: field), "look at @main.rs ", "sent: \(calls.made)")
        screen.type("please", into: field)
        screen.pressReturn(in: field)
        screen.wait("the mention is sent") { calls.made.last == "look at @main.rs please | file:///work/main.rs" }

        // The daemon says no: the text is back, with the reason.
        calls.refuse = "the session log cannot be written"
        screen.type("again", into: field)
        screen.pressReturn(in: field)
        screen.wait("the refusal is shown") { screen.shows("Not sent: the session log cannot be written") }
        XCTAssertEqual(screen.text(in: field), "again")
        calls.refuse = nil
        screen.press("Send")
        screen.wait("it is sent now") { calls.made.suffix(2) == ["again", "again"] }
        XCTAssertFalse(screen.shows("Not sent"))
        XCTAssertFalse(screen.buttons.contains("Stop"))
        XCTAssertFalse(screen.buttons.contains("Attach"), "this agent takes no files")
    }

    func testARunningTurnCanBeStoppedAndAnAgentThatTakesFilesGetsAttach() throws {
        let calls = Calls()
        let screen = Screen {
            Composer(running: true, send: { _ in }, cancel: { try? calls.note("cancel") }, accepts: Accepts(image: true))
        }
        XCTAssertTrue(screen.buttons.contains("Attach"))
        XCTAssertEqual(screen.text(in: "Add to the running turn…"), "")
        screen.press("Stop")
        XCTAssertEqual(calls.made, ["cancel"])
    }

    func testASwitchIsChangedAndARefusalIsShown() throws {
        let calls = Calls()
        let state = try XCTUnwrap(RenderTests().conversation("claude-v2/simple").root)
        let settings = Setting.all(from: state.values["configOptions"])
        let toggle = try XCTUnwrap(settings.first { $0.current == .on(false) })
        let screen = Screen { SettingsBar(settings: settings) { id, value in try calls.note("\(id)=\(value)") } }
        XCTAssertFalse(screen.isOn(toggle.name))
        screen.press(toggle.name)
        screen.wait("the app is asked") { calls.made == ["\(toggle.id)=on(true)"] }
        // The agent has not said it changed (the settings here are fixed), so it shows off again.
        screen.wait("the switch is free again") { screen.isEnabled(toggle.name) }
        XCTAssertFalse(screen.isOn(toggle.name))

        calls.refuse = "not on this model"
        screen.press(toggle.name)
        screen.wait("the refusal is shown") { screen.shows("Not changed: \(toggle.name): not on this model") }
    }

    func testTheForkButtonSaysWhyItFailedAndWorksTheSecondTime() throws {
        let calls = Calls()
        let screen = Screen { OpenElsewhereNotice(holder: OpenElsewhere(pid: 4242, status: "busy")) { try calls.note("fork") } }
        XCTAssertTrue(screen.shows("(pid 4242, busy)"))
        calls.refuse = "the agent cannot fork"
        screen.press("Fork and continue here")
        screen.wait("the reason is shown") { screen.shows("Could not fork: the agent cannot fork") }
        calls.refuse = nil
        screen.press("Fork and continue here")
        screen.wait("the fork is asked for again") { calls.made == ["fork", "fork"] }
        XCTAssertFalse(screen.shows("Could not fork"))
    }

    func testFoldsOpenAndAMissingAgentCanBeTriedAgain() throws {
        let patch = (["--- a/long.txt", "+++ b/long.txt", "@@ -1,40 +1,40 @@", "-first", "+FIRST"]
            + (2...39).map { " line \($0)" } + ["-last", "+LAST", ""]).joined(separator: "\n")
        let diff = Screen(height: 1400) { DiffView(block: ["type": "diff", "patch": ["text": .string(patch)], "changes": [["path": "long.txt", "operation": "modify"]]]) }
        XCTAssertFalse(diff.shows("line 20"))
        diff.press(try XCTUnwrap(diff.buttons.first { $0.hasSuffix("unchanged lines") }))
        XCTAssertTrue(diff.shows("line 20"))
        XCTAssertFalse(diff.buttons.contains { $0.hasSuffix("unchanged lines") })

        let thinking = try RenderTests().conversation("claude-v2/thinking")
        let thought = try XCTUnwrap(thinking.root?.collections["agentThought"]?.values.first)
        let words = contentText(thought["content"]).split(separator: " ").prefix(3).joined(separator: " ")
        let folded = Screen { ThoughtView(item: thought) }
        XCTAssertFalse(folded.shows(words))
        folded.press("Thinking")
        XCTAssertTrue(folded.shows(words), "\(folded.texts)")

        let calls = Calls()
        let agents = Screen { AgentStatus(agents: [["agentId": "pi", "name": "Pi", "readiness": "not_installed"]]) { try? calls.note("retry") } }
        XCTAssertTrue(agents.shows("Pi is not installed"))
        agents.press("Try again")
        XCTAssertEqual(calls.made, ["retry"])
    }

    func testASessionIsChosenFromTheListAndMoreAreAskedFor() throws {
        let calls = Calls()
        let sessions: [SessionSummary] = [
            ["sessionId": "s1", "agentId": "claude", "cwd": "/work/app", "title": "Fix the login", "state": "requires_action"],
            ["sessionId": "s2", "agentId": "claude", "cwd": "/work/site", "title": "Docs", "state": "running"],
        ].compactMap { SessionSummary($0) }
        let screen = Screen { SessionList(sessions: sessions, selection: .constant("s2"), more: { try? calls.note("more") }) }
        XCTAssertTrue(screen.shows("Fix the login") && screen.shows("needs you") && screen.shows("Docs"), "\(screen.texts)")
        // A list does not report its rows in the order they are drawn.
        XCTAssertEqual(screen.texts.filter { ["app", "site"].contains($0) }.sorted(), ["app", "site"], "one heading per folder")
        screen.press("Show more")
        screen.wait("more are asked for") { calls.made == ["more"] }
    }
}
