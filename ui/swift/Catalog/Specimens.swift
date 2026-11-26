import AppKit
import ArchipelClient
import ArchipelUI
import SwiftUI

// Every view of ArchipelUI in each of its states, with the same made-up data and the same
// sections as the React catalogue (ui/react/demo/catalog.tsx), so the two can be compared.

struct Specimen: Identifiable {
    var name: String
    /// How the app writes it.
    var code: String
    var note: String?
    /// Takes the whole width of the page.
    var wide = false
    /// The width it is drawn at, in points.
    var width: CGFloat = 520
    var draw: @MainActor () -> AnyView
    var id: String { name }
}

struct CatalogSection: Identifiable {
    var id: String
    var title: String
    var intro: String
    var specimens: [Specimen]
}

// ---- Made-up data ----

private func text(_ body: String) -> JSON { [["type": "text", "text": .string(body)]] }
private func b64(_ text: String) -> String { Data(text.utf8).base64EncodedString() }

private let markdown = """
I found the cause. The session list sorts by **creation time**, not by the last update, so a session that was just answered stays at the bottom.

### What changes

1. `byFolder` keeps the order the daemon sends.
2. The daemon sends sessions by `updatedAt`, newest first.

| File | Change |
|---|---|
| `sessions.rs` | sort by `updated_at` |
| `sessions.tsx` | none |

> The Swift kit had the same bug and gets the same fix.

```rust
sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
```

See [the design note](https://example.com/notes) for why.
"""

private let patch = """
diff --git a/src/sessions.rs b/src/sessions.rs
--- a/src/sessions.rs
+++ b/src/sessions.rs
@@ -40,14 +40,14 @@ impl Sessions {
     pub fn list(&self) -> Vec<Summary> {
         let mut sessions: Vec<Summary> = self
             .by_id
             .values()
             .map(Summary::from)
             .collect();
-        sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));
+        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
         sessions
     }

     pub fn get(&self, id: &str) -> Option<&Session> {
         self.by_id.get(id)
     }
-
-    // TODO: order
+    /// Newest first.
 }
"""

private let newFile = """
diff --git a/notes.md b/notes.md
new file mode 100644
--- /dev/null
+++ b/notes.md
@@ -0,0 +1,3 @@
+# Notes
+
+The list is sorted by the last update.
"""

private let esc = "\u{1b}"
private let ansi = "\(esc)[1mrunning 3 tests\(esc)[0m\ntest sessions::list_sorts ... \(esc)[32mok\(esc)[0m\ntest sessions::get_missing ... \(esc)[32mok\(esc)[0m\ntest sessions::rename ... \(esc)[31mFAILED\(esc)[0m\n\n\(esc)[33mwarning\(esc)[0m: unused variable: `draft`\n\(esc)[2m  --> src/sessions.rs:88:9\(esc)[0m\n\ntest result: \(esc)[31;1mFAILED\(esc)[0m. 2 passed; 1 failed"

private let terminals: [String: JSON] = [
    "running": ["terminalId": "running", "command": "cargo test --workspace", "output": ["data": .string(b64("   Compiling archipel-server v0.1.0\n   Compiling archipeld v0.1.0"))]],
    "ok": ["terminalId": "ok", "command": "git status --short", "output": ["data": .string(b64(" M crates/archipel-server/src/sessions.rs\n?? notes.md"))], "exitStatus": ["exitCode": 0]],
    "failed": ["terminalId": "failed", "command": "cargo test -p archipel-server sessions", "output": ["data": .string(b64(ansi))], "exitStatus": ["exitCode": 101]],
]

private func tool(_ status: String, _ extra: [String: JSON] = [:]) -> JSON {
    var item: [String: JSON] = ["toolCallId": .string(status), "name": "Read", "kind": "read", "title": "crates/archipel-server/src/sessions.rs", "status": .string(status)]
    for (key, value) in extra { item[key] = value }
    return .object(item)
}

private let planEntries: JSON = [
    ["content": "Find where the list is sorted", "status": "completed", "priority": "high"],
    ["content": "Sort by the last update", "status": "in_progress", "priority": "high"],
    ["content": "Add a test for the order", "status": "pending", "priority": "medium"],
    ["content": "Same fix in the Swift kit", "status": "pending", "priority": "low"],
]

private let permissionParams: JSON = [
    "subject": ["type": "tool_call", "toolCall": ["toolCallId": "rm", "title": "rm -rf target/debug/incremental", "kind": "execute"]],
    "title": "rm -rf target/debug/incremental",
    "options": [
        ["optionId": "allow-once", "name": "Allow once", "kind": "allow_once"],
        ["optionId": "allow-always", "name": "Always allow", "kind": "allow_always"],
        ["optionId": "reject", "name": "Reject", "kind": "reject_once"],
    ],
]

private let questionParams: JSON = [
    "message": "Which kit should get the fix first?",
    "requestedSchema": [
        "type": "object",
        "properties": [
            "kit": ["type": "string", "title": "Kit", "oneOf": [["const": "react", "title": "React"], ["const": "swift", "title": "SwiftUI"]]],
            "note": ["type": "string", "title": "Anything else?"],
        ],
    ],
]

/// A request as the client holds it: asked, and answered when `outcome` is given.
private func request(_ method: String, _ params: JSON, outcome: JSON? = nil) -> Request {
    var state = SessionState()
    do { try state.applyRequest(id: "r", method: method, params: params) } catch { fatalError("\(method): \(error)") }
    if let outcome { _ = state.resolveRequest(id: "r", outcome: outcome) }
    return state.requests["r"]!
}

private let configOptions: JSON = [
    ["configId": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "sonnet", "options": [
        ["value": "haiku", "name": "Haiku"], ["value": "sonnet", "name": "Sonnet"], ["value": "opus", "name": "Opus"],
    ]],
    ["configId": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": "medium", "options": [
        ["value": "low", "name": "Low"], ["value": "medium", "name": "Medium"], ["value": "high", "name": "High"],
    ]],
    ["configId": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "default", "options": [
        ["value": "default", "name": "Ask before edits"], ["value": "acceptEdits", "name": "Accept edits"], ["value": "plan", "name": "Plan"],
    ]],
    ["configId": "fast", "name": "Fast mode", "type": "boolean", "currentValue": false],
]

private let commands: JSON = [
    ["name": "review", "description": "Review the current diff"],
    ["name": "compact", "description": "Summarise the conversation to free context", "input": ["hint": "[instructions]"]],
    ["name": "init", "description": "Write a CLAUDE.md for this project"],
]

private func usage(_ used: Int, _ cost: Double? = nil) -> JSON {
    var value: [String: JSON] = ["used": .number(Double(used)), "size": 200_000]
    if let cost { value["cost"] = ["amount": .number(cost), "currency": "USD"] }
    return .object(value)
}

private func state(_ values: [String: JSON], _ collections: [String: [String: JSON]] = [:], _ timeline: [(String, String)] = []) -> SessionState {
    var made = SessionState()
    made.values = values
    made.collections = collections
    made.timeline = timeline.map { ItemRef(target: $0.0, id: $0.1) }
    return made
}

private let now = Date()
private func minutesAgo(_ minutes: Double) -> String { ISO8601DateFormatter().string(from: now.addingTimeInterval(-minutes * 60)) }
private let sessions: [SessionSummary] = [
    ["sessionId": "a", "agentId": "claude", "title": "Sort the session list by last update", "cwd": "/work/archipel", "state": "requires_action", "updatedAt": .string(minutesAgo(0))],
    ["sessionId": "b", "agentId": "claude", "title": "Fix the diff fold in dark mode", "cwd": "/work/archipel", "state": "running", "updatedAt": .string(minutesAgo(4))],
    ["sessionId": "c", "agentId": "claude", "title": "Write the addon coverage script", "cwd": "/work/archipel", "state": "idle", "updatedAt": .string(minutesAgo(180))],
    ["sessionId": "d", "agentId": "claude", "title": "Review the launchd plist", "cwd": "/work/archipel", "openElsewhere": true, "state": "idle", "updatedAt": .string(minutesAgo(60 * 26))],
    ["sessionId": "e", "agentId": "pi", "title": "Port the timeline to SwiftUI", "cwd": "/work/harbor-app", "logged": false, "updatedAt": .string(minutesAgo(60 * 24 * 6))],
    ["sessionId": "f", "agentId": "pi", "cwd": "/work/harbor-app", "state": "idle", "updatedAt": .string(minutesAgo(60 * 24 * 40))],
].compactMap { SessionSummary($0) }

/// A small picture, as a user would attach one: drawn here, sent as its bytes.
private let picture: String = {
    let image = NSImage(size: NSSize(width: 160, height: 100), flipped: false) { rect in
        NSGradient(starting: NSColor(red: 0.43, green: 0.63, blue: 1, alpha: 1), ending: NSColor(red: 0.82, green: 0.66, blue: 1, alpha: 1))?.draw(in: rect, angle: -30)
        NSColor.white.withAlphaComponent(0.8).setFill()
        NSBezierPath(roundedRect: NSRect(x: 16, y: 74, width: 80, height: 10), xRadius: 3, yRadius: 3).fill()
        NSColor.white.withAlphaComponent(0.5).setFill()
        NSBezierPath(roundedRect: NSRect(x: 16, y: 58, width: 120, height: 8), xRadius: 3, yRadius: 3).fill()
        NSBezierPath(roundedRect: NSRect(x: 16, y: 44, width: 100, height: 8), xRadius: 3, yRadius: 3).fill()
        return true
    }
    let rep = NSBitmapImageRep(data: image.tiffRepresentation!)!
    return rep.representation(using: .png, properties: [:])!.base64EncodedString()
}()

/// A small conversation with a subagent and a request waiting, for the Subagent and ChatView specimens.
private let conversation: Conversation = {
    var made = Conversation()
    let child = state(["state": ["state": "idle"]], [
        "sessionMessage": ["task": ["content": text("Read package.json and reply with its name field.")]],
        "toolCall": ["read": tool("completed", ["content": [["type": "content", "content": ["type": "text", "text": "{ \"name\": \"archipel\" }"]]]])],
        "agentMessage": ["answer": ["content": text("archipel")]],
    ], [("sessionMessage", "task"), ("toolCall", "read"), ("agentMessage", "answer")])
    var root = state([
        "state": ["state": "requires_action", "background": [["terminalId": "running"]]],
        "sessionInfo": ["title": "Sort the session list by last update"],
        "usage": usage(61_000, 0.0412),
        "configOptions": configOptions,
        "availableCommands": commands,
    ], [
        "userMessage": ["u1": ["content": text("The session list puts answered sessions at the bottom. Can you fix it?")]],
        "agentThought": ["t1": ["content": text("The list is probably sorted on the daemon side. Let me look at sessions.rs first.")]],
        "subagent": ["sub": ["sessionId": "sub", "title": "Read package.json name field", "state": ["state": "idle"]]],
        "toolCall": [
            "edit": ["toolCallId": "edit", "name": "Edit", "kind": "edit", "title": "Edit crates/archipel-server/src/sessions.rs", "status": "completed",
                     "content": [["type": "diff", "changes": [["path": "src/sessions.rs", "operation": "modify"]], "patch": ["text": .string(patch)]]]],
            "test": ["toolCallId": "test", "name": "Bash", "kind": "execute", "title": "cargo test --workspace", "status": "in_progress",
                     "content": [["type": "terminal", "terminalId": "running"]]],
        ],
        "terminal": ["running": terminals["running"]!],
        "plan": ["p1": ["entries": planEntries]],
        "agentMessage": ["a1": ["content": text("The list is sorted by creation time. I changed it to the last update and am running the tests.")]],
    ], [("userMessage", "u1"), ("agentThought", "t1"), ("subagent", "sub"), ("plan", "p1"), ("toolCall", "edit"), ("agentMessage", "a1"), ("toolCall", "test"), ("request", "r1")])
    try? root.applyRequest(id: "r1", method: "session/request_permission", params: permissionParams)
    made.sessions = ["root": root, "sub": child]
    made.order = ["root", "sub"]
    return made
}()

private func agent(_ id: String, _ name: String, _ readiness: String, _ detail: String? = nil) -> JSON {
    var value: [String: JSON] = ["agentId": .string(id), "name": .string(name), "readiness": .string(readiness)]
    if let detail { value["detail"] = .string(detail) }
    return .object(value)
}

// ---- Specimens ----

@MainActor
private func view(_ content: some View) -> AnyView { AnyView(content) }

@MainActor
let catalog: [CatalogSection] = [
    CatalogSection(id: "messages", title: "Messages", intro: "What the user and the agent say. The agent's text is Markdown, drawn as SwiftUI views.", specimens: [
        Specimen(name: "UserMessage", code: "UserMessageView(item: item)", note: "Text as written, on the user's side.") {
            view(UserMessageView(item: ["content": text("The session list puts answered sessions at the bottom. Can you fix it?")]))
        },
        Specimen(name: "UserMessage · with attachments", code: "UserMessageView(item: item)", note: "A picture the user sent is shown from its own bytes; files and mentions are named.") {
            view(UserMessageView(item: ["content": [
                ["type": "text", "text": "This is what it looks like now."],
                ["type": "image", "mimeType": "image/png", "data": .string(picture)],
                ["type": "resource_link", "uri": "file:///work/archipel/ui/react/src/sessions.tsx", "name": "sessions.tsx"],
                ["type": "resource", "resource": ["uri": "file:///tmp/order.md", "text": "…"], "name": "order.md"],
            ]]))
        },
        Specimen(name: "AssistantMessage", code: "AssistantMessageView(item: item)", note: "Headings, lists, tables, quotes, code, links. No syntax highlighting.") {
            view(AssistantMessageView(item: ["content": text(markdown)]))
        },
        Specimen(name: "AssistantMessage · short", code: "AssistantMessageView(item: item)") {
            view(AssistantMessageView(item: ["content": text("Done. The tests pass.")]))
        },
        Specimen(name: "Thought · folded", code: "ThoughtView(item: item)", note: "Reasoning, folded by default.") {
            view(ThoughtView(item: ["content": text("The list is probably sorted on the daemon side.")]))
        },
        Specimen(name: "Thought · open", code: "ThoughtView(item: item, open: true)") {
            view(ThoughtView(item: ["content": text("The list is probably sorted on the daemon side. `byFolder` keeps the order it is given, so the fix belongs in **sessions.rs**.")], open: true))
        },
        Specimen(name: "SessionMessage", code: "SessionMessageView(item: item)", note: "A task given to a subagent, or the result it returned.") {
            view(SessionMessageView(item: ["content": text("Read package.json and reply with only the value of its name field.")]))
        },
    ]),
    CatalogSection(id: "tools", title: "Tool calls", intro: "A tool call has a kind, a title, a status badge and what it produced: text, a diff, or a terminal.", specimens: [
        Specimen(name: "ToolCall · pending", code: "ToolCallView(item: item)") { view(ToolCallView(item: tool("pending"))) },
        Specimen(name: "ToolCall · in progress", code: "ToolCallView(item: item)") { view(ToolCallView(item: tool("in_progress"))) },
        Specimen(name: "ToolCall · completed, with output", code: "ToolCallView(item: item)") {
            view(ToolCallView(item: tool("completed", ["content": [["type": "content", "content": ["type": "text", "text": "pub fn list(&self) -> Vec<Summary> {\n    let mut sessions = …;\n    sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));"]]]])))
        },
        Specimen(name: "ToolCall · failed", code: "ToolCallView(item: item)") {
            view(ToolCallView(item: tool("failed", ["content": [["type": "content", "content": ["type": "text", "text": "File does not exist: crates/archipel-server/src/session.rs"]]]])))
        },
        Specimen(name: "ToolCall · cancelled", code: "ToolCallView(item: item)") {
            view(ToolCallView(item: ["toolCallId": "s", "name": "Grep", "kind": "search", "title": "Search \"sort_by\" in crates", "status": "cancelled"]))
        },
        Specimen(name: "ToolCall · with a diff", code: "ToolCallView(item: item)", wide: true, width: 900) {
            view(ToolCallView(item: ["toolCallId": "e", "name": "Edit", "kind": "edit", "title": "Edit crates/archipel-server/src/sessions.rs", "status": "completed",
                                     "content": [["type": "diff", "changes": [["path": "src/sessions.rs", "operation": "modify"]], "patch": ["text": .string(patch)]]]]))
        },
        Specimen(name: "ToolCall · with a terminal", code: "ToolCallView(item: item) { terminals[$0] }", wide: true, width: 900) {
            view(ToolCallView(item: ["toolCallId": "t", "name": "Bash", "kind": "execute", "title": "cargo test -p archipel-server sessions", "status": "failed",
                                     "content": [["type": "terminal", "terminalId": "failed"]]]) { terminals[$0] })
        },
    ]),
    CatalogSection(id: "output", title: "Terminal and diffs", intro: "Command output keeps its colours (the theme's 16 terminal colours). Diffs are per file, with line numbers and folded context.", specimens: [
        Specimen(name: "Terminal · running", code: "TerminalView(item: item)", wide: true, width: 900) { view(TerminalView(item: terminals["running"]!)) },
        Specimen(name: "Terminal · exit 0", code: "TerminalView(item: item)", wide: true, width: 900) { view(TerminalView(item: terminals["ok"]!)) },
        Specimen(name: "Terminal · failed, with colours", code: "TerminalView(item: item)", wide: true, width: 900) { view(TerminalView(item: terminals["failed"]!)) },
        Specimen(name: "Diff", code: "DiffView(block: block)", note: "Added and removed counts, line numbers, a hunk header, and \"Show N unchanged lines\" for long context.", wide: true, width: 900) {
            view(DiffView(block: ["type": "diff", "changes": [["path": "src/sessions.rs", "operation": "modify"]], "patch": ["text": .string(patch)]]))
        },
        Specimen(name: "Diff · new file", code: "DiffView(block: block)") {
            view(DiffView(block: ["type": "diff", "changes": [["path": "notes.md", "operation": "add"]], "patch": ["text": .string(newFile)]]))
        },
    ]),
    CatalogSection(id: "structure", title: "Plans, subagents, compaction", intro: "Items that hold other content.", specimens: [
        Specimen(name: "Plan · entries", code: "PlanView(item: item)", note: "Done (a tick, struck through), in progress (a dot), pending (a ring). Todos arrive the same way.") {
            view(PlanView(item: ["entries": planEntries]))
        },
        Specimen(name: "Plan · document", code: "PlanView(item: item)") {
            view(PlanView(item: ["content": text("1. Find where the list is sorted.\n2. Sort by the last update.\n3. Add a test.")]))
        },
        Specimen(name: "Subagent · running", code: "SubagentView(item: item) { … }") {
            view(SubagentView(item: ["sessionId": "x", "title": "Look for every place that sorts sessions", "state": ["state": "running"]]) { EmptyView() })
        },
        Specimen(name: "Subagent · done, with its timeline", code: "SubagentView(item: item) { SessionTimeline(…) }", note: "Not a card: its timeline hangs from its row on a guide line.", wide: true, width: 900) {
            view(SubagentView(item: conversation.sessions["root"]!.collections["subagent"]!["sub"]!) {
                SessionTimeline(conversation: conversation, state: conversation.sessions["sub"]!)
            })
        },
        Specimen(name: "Compaction · folded", code: "CompactionView(item: item)") {
            view(CompactionView(item: ["status": "completed", "summary": text("The user asked to sort the session list by last update. The daemon now sorts by updated_at; tests pass.")]))
        },
        Specimen(name: "Compaction · open", code: "CompactionView(item: item, open: true)") {
            view(CompactionView(item: ["status": "completed", "summary": text("The user asked to sort the session list by last update. The daemon now sorts by updated_at; tests pass.")], open: true))
        },
        Specimen(name: "Fallback", code: "FallbackView(kind: kind, item: item)", note: "Any kind the kit does not know, such as an extension's x.acme.checkpoint.") {
            view(FallbackView(kind: "x.acme.checkpoint", item: ["label": "before refactor", "commit": "9f4b4ad"]))
        },
        Specimen(name: "Fallback · open", code: "FallbackView(kind: kind, item: item, open: true)") {
            view(FallbackView(kind: "x.acme.checkpoint", item: ["label": "before refactor", "commit": "9f4b4ad"], open: true))
        },
    ]),
    CatalogSection(id: "requests", title: "Approvals and questions", intro: "The turn waits on these. Pending ones have the attention colour on their border; once answered they show the answer.", specimens: [
        Specimen(name: "Approval · pending", code: "ApprovalView(request: request) { … }", note: "The first option is the primary button.") {
            view(ApprovalView(request: request("session/request_permission", permissionParams)))
        },
        Specimen(name: "Approval · answered", code: "ApprovalView(request: request)") {
            view(ApprovalView(request: request("session/request_permission", permissionParams, outcome: ["outcome": ["outcome": "selected", "optionId": "allow-once"]])))
        },
        Specimen(name: "Approval · cancelled", code: "ApprovalView(request: request)") {
            view(ApprovalView(request: request("session/request_permission", permissionParams, outcome: ["outcome": ["outcome": "cancelled"]])))
        },
        Specimen(name: "Question · pending", code: "QuestionView(request: request) { … }", note: "A form built from the schema the agent sent: a menu for choices, a field for text.") {
            view(QuestionView(request: request("session/elicitation", questionParams)))
        },
        Specimen(name: "Question · answered", code: "QuestionView(request: request)") {
            view(QuestionView(request: request("session/elicitation", questionParams, outcome: ["action": "accept", "content": ["kit": "react", "note": "then Swift"]])))
        },
        Specimen(name: "Question · skipped", code: "QuestionView(request: request)") {
            view(QuestionView(request: request("session/elicitation", questionParams, outcome: ["action": "decline"])))
        },
    ]),
    CatalogSection(id: "chrome", title: "Around the timeline", intro: "The status bar at the top of a chat, and what sits above the composer.", specimens: [
        Specimen(name: "StatusBar · idle", code: "StatusBar(state: state)", wide: true, width: 900) {
            view(StatusBar(state: state(["state": ["state": "idle"], "sessionInfo": ["title": "Sort the session list by last update"], "usage": usage(23_000, 0.0123)])))
        },
        Specimen(name: "StatusBar · running", code: "StatusBar(state: state)", wide: true, width: 900) {
            view(StatusBar(state: state(["state": ["state": "running"], "sessionInfo": ["title": "Fix the diff fold in dark mode"], "usage": usage(92_000, 0.31)])))
        },
        Specimen(name: "StatusBar · needs action, context nearly full", code: "StatusBar(state: state)", note: "The meter turns to the attention colour at 80%.", wide: true, width: 900) {
            view(StatusBar(state: state(["state": ["state": "requires_action"], "sessionInfo": ["title": "Review the launchd plist"], "usage": usage(171_000, 2.4)])))
        },
        Specimen(name: "StatusBar · new session, no usage", code: "StatusBar(state: state)", wide: true, width: 900) {
            view(StatusBar(state: state(["state": ["state": "idle"]])))
        },
        Specimen(name: "Usage · context only (Pi)", code: "UsageView(state: state)", note: "Pi gives no cost.") {
            view(UsageView(state: state(["usage": usage(48_000)])))
        },
        Specimen(name: "Activity · tasks and background work", code: "ActivityView(state: state)", wide: true, width: 900) {
            view(ActivityView(state: state(["state": ["state": "idle", "background": [["terminalId": "running"], ["sessionId": "x"]]]],
                                            ["plan": ["p": ["entries": planEntries]], "terminal": ["running": terminals["running"]!], "subagent": ["x": ["title": "Find every place that sorts sessions"]]],
                                            [("plan", "p")])))
        },
        Specimen(name: "Activity · tasks open", code: "ActivityView(state: state, open: true)", wide: true, width: 900) {
            view(ActivityView(state: state([:], ["plan": ["p": ["entries": planEntries]]], [("plan", "p")]), open: true))
        },
        Specimen(name: "OpenElsewhere · with fork", code: "OpenElsewhereNotice(holder: holder) { … }", wide: true, width: 900) {
            view(OpenElsewhereNotice(holder: OpenElsewhere(pid: 48213, status: "busy")) {})
        },
        Specimen(name: "OpenElsewhere · without fork", code: "OpenElsewhereNotice(holder: holder)", wide: true, width: 900) {
            view(OpenElsewhereNotice(holder: OpenElsewhere(pid: 48213)))
        },
    ]),
    CatalogSection(id: "input", title: "Settings and composer", intro: "The settings come from what the agent reports: a menu per choice, a switch per on/off setting.", specimens: [
        Specimen(name: "SettingsBar", code: "SettingsBar(settings: Setting.all(from: configOptions)) { … }", wide: true, width: 900) {
            view(SettingsBar(settings: Setting.all(from: configOptions)) { _, _ in })
        },
        Specimen(name: "SettingsBar · read only", code: "SettingsBar(settings: settings)", note: "Without change, or while a change is being confirmed.", wide: true, width: 900) {
            view(SettingsBar(settings: Setting.all(from: configOptions)))
        },
        Specimen(name: "Composer · idle", code: "Composer(running: false, send: { … })", wide: true, width: 900) {
            view(Composer(running: false, send: { _ in }))
        },
        Specimen(name: "Composer · running", code: "Composer(running: true, send: { … }, cancel: { … })", note: "The placeholder changes and Stop appears.", wide: true, width: 900) {
            view(Composer(running: true, send: { _ in }))
        },
        Specimen(name: "Composer · can attach", code: "Composer(…, accepts: Accepts(image: true, files: true))", wide: true, width: 900) {
            view(Composer(running: false, send: { _ in }, accepts: Accepts(image: true, files: true)))
        },
        Specimen(name: "Composer · command menu", code: "Composer(…, commands: Command.all(from: …), text: \"/\")", note: "After typing \"/\". The chosen row is highlighted; arrows move it.", wide: true, width: 900) {
            view(Composer(running: false, send: { _ in }, commands: Command.all(from: commands), text: "/"))
        },
        Specimen(name: "Composer · mention menu", code: "Composer(…, mentions: { … }, text: \"Look at @sess\")", note: "After typing \"@\", with the files the app supplies.", wide: true, width: 900) {
            view(Composer(running: false, send: { _ in }, mentions: { _ in [
                Mention(uri: "file:///work/archipel/ui/react/src/sessions.tsx", name: "ui/react/src/sessions.tsx", detail: "182 lines"),
                Mention(uri: "file:///work/archipel/crates/archipel-server/src/sessions.rs", name: "crates/archipel-server/src/sessions.rs", detail: "410 lines"),
            ] }, text: "Look at @sess"))
        },
    ]),
    CatalogSection(id: "notices", title: "Agent and connection", intro: "Notices for the app's own chrome. Both draw nothing when all is well.", specimens: [
        Specimen(name: "AgentStatus", code: "AgentStatus(agents: agents) { … }", note: "Three tones: wait (muted), act (attention colour), bad (error colour).", wide: true, width: 900) {
            view(AgentStatus(agents: [
                agent("claude", "Claude Code", "starting"),
                agent("codex", "Codex", "needs_login"),
                agent("pi", "Pi", "not_installed", "pi was not found on PATH"),
                agent("gemini", "Gemini", "error", "exited with code 1"),
            ]) {})
        },
        Specimen(name: "ConnectionStatus · lost", code: "ConnectionStatus(connected: false)", wide: true, width: 900) {
            view(ConnectionStatus(connected: false))
        },
    ]),
    CatalogSection(id: "sessions", title: "Session list", intro: "Grouped by folder. A row says what needs the user first; rename and delete are in the row's context menu.", specimens: [
        Specimen(name: "SessionList", code: "SessionList(sessions: sessions, selection: $selected, …)", note: "needs you · running · open elsewhere · from <agent> (started outside the app) · untitled.", width: 320) {
            view(SessionList(sessions: sessions, selection: .constant("b"), rename: { _, _ in }, delete: { _ in }, more: {}).frame(height: 560))
        },
        Specimen(name: "SessionList · empty", code: "SessionList(sessions: [], selection: $selected)", width: 320) {
            view(SessionList(sessions: [], selection: .constant(nil)).frame(height: 120))
        },
        Specimen(name: "SessionRow", code: "SessionRow(session: session)", note: "One row, for a list of your own.", width: 320) {
            view(VStack(alignment: .leading, spacing: 8) { ForEach(sessions.prefix(4)) { SessionRow(session: $0, now: now) } })
        },
    ]),
    CatalogSection(id: "chat", title: "Assembled: ChatView", intro: "Everything above, put together by ChatView: status, timeline, activity, settings and composer.", specimens: [
        Specimen(name: "ChatView", code: "ChatView(conversation: conversation, actions: actions)", wide: true, width: 900) {
            var actions = ChatActions()
            actions.prompt = { _ in }
            actions.setOption = { _, _ in }
            return view(ChatView(conversation: conversation, actions: actions).frame(height: 1500))
        },
    ]),
]
