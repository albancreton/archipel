//! The catalogue's sections and specimens, and the made-up data they draw.

use archipel_core::state::{Conversation, ItemRef, Request, SessionState};
use archipel_gpui::composer::{Accepts, Command, Composer, FindMentions, Mention, Send};
use archipel_gpui::sessions::{SessionList, SessionSummary, now, session_row};
use archipel_gpui::settings::{Setting, settings_bar};
use archipel_gpui::status::{
    OpenElsewhere, activity, agent_status, connection_status, open_elsewhere, status_bar, usage,
};
use archipel_gpui::{ChatActions, ChatView, ItemViews, diff, items, memory, timeline};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{App, AppContext as _, Task, Window, div, prelude::*, px};
use serde_json::{Value, json};
use std::rc::Rc;

pub struct Specimen {
    pub name: &'static str,
    /// How the app writes it.
    pub code: &'static str,
    pub note: Option<&'static str>,
    /// The width it is drawn at, and its height when it fills what it is given.
    pub width: f32,
    pub height: Option<f32>,
    pub draw: archipel_gpui::parts::Draw,
}

pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub intro: &'static str,
    pub specimens: Vec<Specimen>,
}

fn specimen<E: IntoElement>(
    name: &'static str,
    code: &'static str,
    draw: impl Fn(&mut Window, &mut App) -> E + 'static,
) -> Specimen {
    Specimen {
        name,
        code,
        note: None,
        width: 520.0,
        height: None,
        draw: Box::new(move |window, cx| draw(window, cx).into_any_element()),
    }
}

impl Specimen {
    fn note(mut self, note: &'static str) -> Self {
        self.note = Some(note);
        self
    }
    fn wide(mut self) -> Self {
        self.width = 900.0;
        self
    }
    fn sized(mut self, width: f32, height: Option<f32>) -> Self {
        (self.width, self.height) = (width, height);
        self
    }
}

// ---- Made-up data ----

fn text(body: &str) -> Value {
    json!([{ "type": "text", "text": body }])
}

fn b64(text: &str) -> String {
    BASE64.encode(text)
}

const MARKDOWN: &str = "I found the cause. The session list sorts by **creation time**, not by the last update, so a session that was just answered stays at the bottom.

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

See [the design note](https://example.com/notes) for why.";

const PATCH: &str = "diff --git a/src/sessions.rs b/src/sessions.rs
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
";

const NEW_FILE: &str = "diff --git a/notes.md b/notes.md
new file mode 100644
--- /dev/null
+++ b/notes.md
@@ -0,0 +1,3 @@
+# Notes
+
+The list is sorted by the last update.
";

const ANSI: &str = "\u{1b}[1mrunning 3 tests\u{1b}[0m\ntest sessions::list_sorts ... \u{1b}[32mok\u{1b}[0m\ntest sessions::get_missing ... \u{1b}[32mok\u{1b}[0m\ntest sessions::rename ... \u{1b}[31mFAILED\u{1b}[0m\n\n\u{1b}[33mwarning\u{1b}[0m: unused variable: `draft`\n\u{1b}[2m  --> src/sessions.rs:88:9\u{1b}[0m\n\ntest result: \u{1b}[31;1mFAILED\u{1b}[0m. 2 passed; 1 failed";

fn terminals() -> Value {
    json!({
        "running": { "terminalId": "running", "command": "cargo test --workspace", "output": { "data": b64("   Compiling archipel-server v0.1.0\n   Compiling archipeld v0.1.0") } },
        "ok": { "terminalId": "ok", "command": "git status --short", "output": { "data": b64(" M crates/archipel-server/src/sessions.rs\n?? notes.md") }, "exitStatus": { "exitCode": 0 } },
        "failed": { "terminalId": "failed", "command": "cargo test -p archipel-server sessions", "output": { "data": b64(ANSI) }, "exitStatus": { "exitCode": 101 } },
    })
}

fn tool(status: &str, content: Option<Value>) -> Value {
    let mut item = json!({ "toolCallId": status, "name": "Read", "kind": "read", "title": "crates/archipel-server/src/sessions.rs", "status": status });
    if let Some(content) = content {
        item["content"] = content;
    }
    item
}

fn output(body: &str) -> Value {
    json!([{ "type": "content", "content": { "type": "text", "text": body } }])
}

fn plan_entries() -> Value {
    json!([
        { "content": "Find where the list is sorted", "status": "completed", "priority": "high" },
        { "content": "Sort by the last update", "status": "in_progress", "priority": "high" },
        { "content": "Add a test for the order", "status": "pending", "priority": "medium" },
        { "content": "Same fix in the Swift kit", "status": "pending", "priority": "low" },
    ])
}

fn permission() -> Value {
    json!({
        "subject": { "type": "tool_call", "toolCall": { "toolCallId": "rm", "title": "rm -rf target/debug/incremental", "kind": "execute" } },
        "title": "rm -rf target/debug/incremental",
        "options": [
            { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
            { "optionId": "allow-always", "name": "Always allow", "kind": "allow_always" },
            { "optionId": "reject", "name": "Reject", "kind": "reject_once" },
        ],
    })
}

fn question() -> Value {
    json!({
        "message": "Which kit should get the fix first?",
        "requestedSchema": { "type": "object", "properties": {
            "kit": { "type": "string", "title": "Kit", "oneOf": [{ "const": "react", "title": "React" }, { "const": "swift", "title": "SwiftUI" }] },
            "note": { "type": "string", "title": "Anything else?" },
        } },
    })
}

fn request(method: &str, params: Value, outcome: Option<Value>) -> Request {
    Request {
        method: method.to_owned(),
        params,
        outcome,
    }
}

fn config_options() -> Value {
    json!([
        { "configId": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "sonnet",
          "options": [{ "value": "haiku", "name": "Haiku" }, { "value": "sonnet", "name": "Sonnet" }, { "value": "opus", "name": "Opus" }] },
        { "configId": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": "medium",
          "options": [{ "value": "low", "name": "Low" }, { "value": "medium", "name": "Medium" }, { "value": "high", "name": "High" }] },
        { "configId": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "default",
          "options": [{ "value": "default", "name": "Ask before edits" }, { "value": "acceptEdits", "name": "Accept edits" }, { "value": "plan", "name": "Plan" }] },
        { "configId": "fast", "name": "Fast mode", "type": "boolean", "currentValue": false },
    ])
}

fn commands() -> Value {
    json!([
        { "name": "review", "description": "Review the current diff" },
        { "name": "compact", "description": "Summarise the conversation to free context", "input": { "hint": "[instructions]" } },
        { "name": "init", "description": "Write a CLAUDE.md for this project" },
    ])
}

fn used(tokens: u64, cost: Option<f64>) -> Value {
    let mut value = json!({ "used": tokens, "size": 200_000 });
    if let Some(cost) = cost {
        value["cost"] = json!({ "amount": cost, "currency": "USD" });
    }
    value
}

/// A session's state from its values, its collections (by target, then id) and its timeline.
fn state(values: Value, collections: Value, timeline: &[(&str, &str)]) -> SessionState {
    let mut made = SessionState::default();
    for (name, value) in values.as_object().into_iter().flatten() {
        made.values.insert(name.clone(), value.clone());
    }
    for (target, entries) in collections.as_object().into_iter().flatten() {
        for (id, item) in entries.as_object().into_iter().flatten() {
            made.collections
                .entry(target.clone())
                .or_default()
                .insert(id.clone(), item.clone());
        }
    }
    made.timeline = timeline
        .iter()
        .map(|(target, id)| ItemRef {
            target: (*target).to_owned(),
            id: (*id).to_owned(),
        })
        .collect();
    made
}

/// A time some minutes ago, as the daemon writes it.
fn minutes_ago(minutes: i64) -> String {
    let at = now() - minutes * 60;
    let (days, rest) = (at.div_euclid(86_400), at.rem_euclid(86_400));
    // The civil date from the days, by Howard Hinnant's method.
    let z = days + 719_468;
    let (era, day_of_era) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn sessions() -> Vec<SessionSummary> {
    [
        json!({ "sessionId": "a", "agentId": "claude", "title": "Sort the session list by last update", "cwd": "/work/archipel", "state": "requires_action", "updatedAt": minutes_ago(0) }),
        json!({ "sessionId": "b", "agentId": "claude", "title": "Fix the diff fold in dark mode", "cwd": "/work/archipel", "state": "running", "updatedAt": minutes_ago(4) }),
        json!({ "sessionId": "c", "agentId": "claude", "title": "Write the addon coverage script", "cwd": "/work/archipel", "state": "idle", "updatedAt": minutes_ago(180) }),
        json!({ "sessionId": "d", "agentId": "claude", "title": "Review the launchd plist", "cwd": "/work/archipel", "openElsewhere": true, "state": "idle", "updatedAt": minutes_ago(60 * 26) }),
        json!({ "sessionId": "e", "agentId": "pi", "title": "Port the timeline to SwiftUI", "cwd": "/work/harbor-app", "logged": false, "updatedAt": minutes_ago(60 * 24 * 6) }),
        json!({ "sessionId": "f", "agentId": "pi", "cwd": "/work/harbor-app", "state": "idle", "updatedAt": minutes_ago(60 * 24 * 40) }),
    ]
    .iter()
    .filter_map(SessionSummary::of)
    .collect()
}

/// A small conversation with a subagent and a request waiting, for the Subagent and ChatView specimens.
fn conversation() -> Conversation {
    let child = state(
        json!({ "state": { "state": "idle" } }),
        json!({
            "sessionMessage": { "task": { "content": text("Read package.json and reply with its name field.") } },
            "toolCall": { "read": tool("completed", Some(output("{ \"name\": \"archipel\" }"))) },
            "agentMessage": { "answer": { "content": text("archipel") } },
        }),
        &[
            ("sessionMessage", "task"),
            ("toolCall", "read"),
            ("agentMessage", "answer"),
        ],
    );
    let mut root = state(
        json!({
            "state": { "state": "requires_action", "background": [{ "terminalId": "running" }] },
            "sessionInfo": { "title": "Sort the session list by last update" },
            "usage": used(61_000, Some(0.0412)),
            "configOptions": config_options(),
            "availableCommands": commands(),
        }),
        json!({
            "userMessage": { "u1": { "content": text("The session list puts answered sessions at the bottom. Can you fix it?") } },
            "agentThought": { "t1": { "content": text("The list is probably sorted on the daemon side. Let me look at sessions.rs first.") } },
            "subagent": { "sub": { "sessionId": "sub", "title": "Read package.json name field", "state": { "state": "idle" } } },
            "toolCall": {
                "edit": { "toolCallId": "edit", "name": "Edit", "kind": "edit", "title": "Edit crates/archipel-server/src/sessions.rs", "status": "completed",
                          "content": [{ "type": "diff", "changes": [{ "path": "src/sessions.rs", "operation": "modify" }], "patch": { "text": PATCH } }] },
                "test": { "toolCallId": "test", "name": "Bash", "kind": "execute", "title": "cargo test --workspace", "status": "in_progress",
                          "content": [{ "type": "terminal", "terminalId": "running" }] },
            },
            "terminal": { "running": terminals()["running"] },
            "plan": { "p1": { "entries": plan_entries() } },
            "agentMessage": { "a1": { "content": text("The list is sorted by creation time. I changed it to the last update and am running the tests.") } },
        }),
        &[
            ("userMessage", "u1"),
            ("agentThought", "t1"),
            ("subagent", "sub"),
            ("plan", "p1"),
            ("toolCall", "edit"),
            ("agentMessage", "a1"),
            ("toolCall", "test"),
            ("request", "r1"),
        ],
    );
    root.requests.insert(
        "r1".to_owned(),
        request("session/request_permission", permission(), None),
    );
    let mut made = Conversation::default();
    *made.session_mut("root") = root;
    *made.session_mut("sub") = child;
    made
}

fn agent(id: &str, name: &str, readiness: &str, detail: Option<&str>) -> Value {
    let mut value = json!({ "agentId": id, "name": name, "readiness": readiness });
    if let Some(detail) = detail {
        value["detail"] = json!(detail);
    }
    value
}

/// An action that does nothing and succeeds.
fn done<T>() -> impl Fn(T, &mut Window, &mut App) -> Task<Result<(), String>> {
    |_, _, _| Task::ready(Ok(()))
}

// ---- Specimens ----

pub fn sections(window: &mut Window, cx: &mut App) -> Vec<Section> {
    let send: Send = Rc::new(done());
    let composer =
        |window: &mut Window,
         cx: &mut App,
         setup: &dyn Fn(&mut Composer, &mut gpui::Context<Composer>)| {
            let send = send.clone();
            cx.new(|cx| {
                let mut composer = Composer::new(send, window, cx);
                setup(&mut composer, cx);
                composer
            })
        };
    let idle = composer(window, cx, &|_, _| {});
    let running = composer(window, cx, &|composer, cx| {
        composer.set_cancel(Some(Rc::new(|_, _| {})), cx);
        composer.set_running(true, cx);
    });
    let attaching = composer(window, cx, &|composer, cx| {
        composer.set_accepts(
            Accepts {
                image: true,
                files: true,
            },
            cx,
        )
    });
    let commanding = composer(window, cx, &|composer, cx| {
        composer.set_commands(Command::all(Some(&commands())), cx);
        composer
            .field()
            .clone()
            .update(cx, |field, cx| field.set_text("/", cx));
    });
    let mentioning = composer(window, cx, &|composer, cx| {
        let find: FindMentions = Rc::new(|_, _| {
            Task::ready(vec![
                Mention {
                    uri: "file:///work/archipel/ui/react/src/sessions.tsx".into(),
                    name: "ui/react/src/sessions.tsx".into(),
                    detail: Some("182 lines".into()),
                },
                Mention {
                    uri: "file:///work/archipel/crates/archipel-server/src/sessions.rs".into(),
                    name: "crates/archipel-server/src/sessions.rs".into(),
                    detail: Some("410 lines".into()),
                },
            ])
        });
        composer.set_mentions(Some(find), cx);
        composer
            .field()
            .clone()
            .update(cx, |field, cx| field.set_text("Look at @sess", cx));
    });

    let list = cx.new(|cx| {
        let mut list = SessionList::new(sessions());
        list.set_selected(Some("b".to_owned()), cx);
        list.set_rename(Some(Rc::new(|_, _, _, _| Task::ready(Ok(())))), cx);
        list.set_delete(
            Some(Rc::new(|_, _, _| {
                Task::ready(Err("the daemon wants the admin scope".to_owned()))
            })),
            cx,
        );
        list.set_more(Some(Rc::new(|_, _| {})), cx);
        list
    });
    let empty_list = cx.new(|_| SessionList::new(Vec::new()));

    let chat = cx.new(|cx| {
        let actions = ChatActions {
            approve: Some(Rc::new(|_, _, _, _| {})),
            prompt: Some(Rc::new(done())),
            cancel: Some(Rc::new(|_, _| {})),
            set_option: Some(Rc::new(|_, _, _, _| {
                Task::ready(Err("the agent refused".to_owned()))
            })),
            ..ChatActions::default()
        };
        ChatView::new(conversation(), actions, window, cx)
    });

    // The specimens that are shown open start open.
    for key in [
        "cat/thought-open",
        "cat/compaction-open",
        "cat/fallback-open",
        "cat/activity-open/tasks",
    ] {
        memory::set_flag(cx, key, true);
    }

    let sample = conversation();
    let picture = BASE64.encode(include_bytes!("picture.png"));

    vec![
        Section {
            id: "messages",
            title: "Messages",
            intro: "What the user and the agent say. The agent's text is Markdown, drawn as GPUI elements.",
            specimens: vec![
                specimen("UserMessage", "items::user_message(&item, cx)", |_, cx| {
                    items::user_message(&json!({ "content": text("The session list puts answered sessions at the bottom. Can you fix it?") }), cx)
                })
                .note("Text as written, on the user's side."),
                specimen("UserMessage · with attachments", "items::user_message(&item, cx)", move |_, cx| {
                    items::user_message(
                        &json!({ "content": [
                            { "type": "text", "text": "This is what it looks like now." },
                            { "type": "image", "mimeType": "image/png", "data": picture },
                            { "type": "resource_link", "uri": "file:///work/archipel/ui/react/src/sessions.tsx", "name": "sessions.tsx" },
                            { "type": "resource", "resource": { "uri": "file:///tmp/order.md", "text": "…" }, "name": "order.md" },
                        ] }),
                        cx,
                    )
                })
                .note("A picture the user sent is shown from its own bytes; files and mentions are named."),
                specimen("AssistantMessage", "items::assistant_message(key, &item, window, cx)", |window, cx| {
                    items::assistant_message("cat/markdown", &json!({ "content": text(MARKDOWN) }), window, cx)
                })
                .note("Headings, lists, tables, quotes, code, links. No syntax highlighting."),
                specimen("AssistantMessage · short", "items::assistant_message(key, &item, window, cx)", |window, cx| {
                    items::assistant_message("cat/short", &json!({ "content": text("Done. The tests pass.") }), window, cx)
                }),
                specimen("Thought · folded", "items::thought(key, &item, window, cx)", |window, cx| {
                    items::thought("cat/thought", &json!({ "content": text("The list is probably sorted on the daemon side.") }), window, cx)
                })
                .note("Reasoning, folded by default. Press it to open."),
                specimen("Thought · open", "items::thought(key, &item, window, cx)", |window, cx| {
                    items::thought(
                        "cat/thought-open",
                        &json!({ "content": text("The list is probably sorted on the daemon side. `byFolder` keeps the order it is given, so the fix belongs in **sessions.rs**.") }),
                        window,
                        cx,
                    )
                }),
                specimen("SessionMessage", "items::session_message(&item, cx)", |_, cx| {
                    items::session_message(&json!({ "content": text("Read package.json and reply with only the value of its name field.") }), cx)
                })
                .note("A task given to a subagent, or the result it returned."),
            ],
        },
        Section {
            id: "tools",
            title: "Tool calls",
            intro: "A tool call has a kind, a title, a status badge and what it produced: text, a diff, or a terminal.",
            specimens: vec![
                specimen("ToolCall · pending", "items::tool_call(key, &item, find_terminal, cx)", |_, cx| items::tool_call("cat/t1", &tool("pending", None), |_| None, cx)),
                specimen("ToolCall · in progress", "items::tool_call(key, &item, find_terminal, cx)", |_, cx| items::tool_call("cat/t2", &tool("in_progress", None), |_| None, cx)),
                specimen("ToolCall · completed, with output", "items::tool_call(key, &item, find_terminal, cx)", |_, cx| {
                    let content = output("pub fn list(&self) -> Vec<Summary> {\n    let mut sessions = …;\n    sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));");
                    items::tool_call("cat/t3", &tool("completed", Some(content)), |_| None, cx)
                }),
                specimen("ToolCall · failed", "items::tool_call(key, &item, find_terminal, cx)", |_, cx| {
                    items::tool_call("cat/t4", &tool("failed", Some(output("File does not exist: crates/archipel-server/src/session.rs"))), |_| None, cx)
                }),
                specimen("ToolCall · cancelled", "items::tool_call(key, &item, find_terminal, cx)", |_, cx| {
                    items::tool_call("cat/t5", &json!({ "toolCallId": "s", "name": "Grep", "kind": "search", "title": "Search \"sort_by\" in crates", "status": "cancelled" }), |_| None, cx)
                }),
                specimen("ToolCall · with a diff", "items::tool_call(key, &item, find_terminal, cx)", |_, cx| {
                    let item = json!({ "toolCallId": "e", "name": "Edit", "kind": "edit", "title": "Edit crates/archipel-server/src/sessions.rs", "status": "completed",
                        "content": [{ "type": "diff", "changes": [{ "path": "src/sessions.rs", "operation": "modify" }], "patch": { "text": PATCH } }] });
                    items::tool_call("cat/t6", &item, |_| None, cx)
                })
                .wide(),
                specimen("ToolCall · with a terminal", "items::tool_call(key, &item, |id| terminals.get(id), cx)", |_, cx| {
                    let item = json!({ "toolCallId": "t", "name": "Bash", "kind": "execute", "title": "cargo test -p archipel-server sessions", "status": "failed",
                        "content": [{ "type": "terminal", "terminalId": "failed" }] });
                    let terminals = terminals();
                    items::tool_call("cat/t7", &item, |id| terminals.get(id), cx)
                })
                .wide(),
            ],
        },
        Section {
            id: "output",
            title: "Terminal and diffs",
            intro: "Command output keeps its colours (the theme's 16 terminal colours). Diffs are per file, with line numbers and folded context.",
            specimens: vec![
                specimen("Terminal · running", "items::terminal(&item, cx)", |_, cx| items::terminal(&terminals()["running"], cx)).wide(),
                specimen("Terminal · exit 0", "items::terminal(&item, cx)", |_, cx| items::terminal(&terminals()["ok"], cx)).wide(),
                specimen("Terminal · failed, with colours", "items::terminal(&item, cx)", |_, cx| items::terminal(&terminals()["failed"], cx)).wide(),
                specimen("Diff", "diff::diff(key, &block, cx)", |_, cx| {
                    diff::diff("cat/diff", &json!({ "type": "diff", "changes": [{ "path": "src/sessions.rs", "operation": "modify" }], "patch": { "text": PATCH } }), cx)
                })
                .note("Added and removed counts, line numbers, a hunk header, and \"Show N unchanged lines\" for long context.")
                .wide(),
                specimen("Diff · new file", "diff::diff(key, &block, cx)", |_, cx| {
                    diff::diff("cat/new", &json!({ "type": "diff", "changes": [{ "path": "notes.md", "operation": "add" }], "patch": { "text": NEW_FILE } }), cx)
                }),
            ],
        },
        Section {
            id: "structure",
            title: "Plans, subagents, compaction",
            intro: "Items that hold other content.",
            specimens: vec![
                specimen("Plan · entries", "items::plan(&item, cx)", |_, cx| items::plan(&json!({ "entries": plan_entries() }), cx))
                    .note("Done (a tick, struck through), in progress (a dot), pending (a ring). Todos arrive the same way."),
                specimen("Plan · document", "items::plan(&item, cx)", |_, cx| {
                    items::plan(&json!({ "content": text("1. Find where the list is sorted.\n2. Sort by the last update.\n3. Add a test.") }), cx)
                }),
                specimen("Subagent · running", "items::subagent(&item, None, cx)", |_, cx| {
                    items::subagent(&json!({ "sessionId": "x", "title": "Look for every place that sorts sessions", "state": { "state": "running" } }), None, cx)
                }),
                specimen("Subagent · done, with its timeline", "items::subagent(&item, Some(timeline(…)), cx)", move |window, cx| {
                    let inner = timeline(&sample, "cat-sub", &sample.sessions["sub"], &ChatActions::default(), &ItemViews::new(), window, cx);
                    items::subagent(&sample.sessions["root"].collections["subagent"]["sub"], Some(inner.into_any_element()), cx)
                })
                .note("Not a card: its timeline hangs from its row on a guide line.")
                .wide(),
                specimen("Compaction · folded", "items::compaction(key, &item, cx)", |_, cx| {
                    items::compaction("cat/compaction", &json!({ "status": "completed", "summary": text("The user asked to sort the session list by last update. The daemon now sorts by updated_at; tests pass.") }), cx)
                }),
                specimen("Compaction · open", "items::compaction(key, &item, cx)", |_, cx| {
                    items::compaction("cat/compaction-open", &json!({ "status": "completed", "summary": text("The user asked to sort the session list by last update. The daemon now sorts by updated_at; tests pass.") }), cx)
                }),
                specimen("Fallback", "items::fallback(key, kind, &item, cx)", |_, cx| {
                    items::fallback("cat/fallback", "x.acme.checkpoint", &json!({ "label": "before refactor", "commit": "9f4b4ad" }), cx)
                })
                .note("Any kind the kit does not know, such as an extension's x.acme.checkpoint."),
                specimen("Fallback · open", "items::fallback(key, kind, &item, cx)", |_, cx| {
                    items::fallback("cat/fallback-open", "x.acme.checkpoint", &json!({ "label": "before refactor", "commit": "9f4b4ad" }), cx)
                }),
            ],
        },
        Section {
            id: "requests",
            title: "Approvals and questions",
            intro: "The turn waits on these. Pending ones have the attention colour on their border; once answered they show the answer.",
            specimens: vec![
                specimen("Approval · pending", "items::approval(key, &request, Some(choose), cx)", |_, cx| {
                    items::approval("cat/a1", &request("session/request_permission", permission(), None), Some(Rc::new(|_, _, _| {})), cx)
                })
                .note("The first option is the primary button."),
                specimen("Approval · answered", "items::approval(key, &request, None, cx)", |_, cx| {
                    let outcome = json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } });
                    items::approval("cat/a2", &request("session/request_permission", permission(), Some(outcome)), None, cx)
                }),
                specimen("Approval · cancelled", "items::approval(key, &request, None, cx)", |_, cx| {
                    let outcome = json!({ "outcome": { "outcome": "cancelled" } });
                    items::approval("cat/a3", &request("session/request_permission", permission(), Some(outcome)), None, cx)
                }),
                specimen("Question · pending", "items::question(key, &request, Some(answer), window, cx)", |window, cx| {
                    items::question("cat/q1", &request("session/elicitation", question(), None), Some(Rc::new(|_, _, _| {})), window, cx)
                })
                .note("A form built from the schema the agent sent: a menu for choices, a field for text."),
                specimen("Question · answered", "items::question(key, &request, None, window, cx)", |window, cx| {
                    let outcome = json!({ "action": "accept", "content": { "kit": "react", "note": "then Swift" } });
                    items::question("cat/q2", &request("session/elicitation", question(), Some(outcome)), None, window, cx)
                }),
                specimen("Question · skipped", "items::question(key, &request, None, window, cx)", |window, cx| {
                    items::question("cat/q3", &request("session/elicitation", question(), Some(json!({ "action": "decline" }))), None, window, cx)
                }),
            ],
        },
        Section {
            id: "chrome",
            title: "Around the timeline",
            intro: "The status bar at the top of a chat, and what sits above the composer.",
            specimens: vec![
                specimen("StatusBar · idle", "status::status_bar(&state, cx)", |_, cx| {
                    status_bar(&state(json!({ "state": { "state": "idle" }, "sessionInfo": { "title": "Sort the session list by last update" }, "usage": used(23_000, Some(0.0123)) }), json!({}), &[]), cx)
                })
                .wide(),
                specimen("StatusBar · running", "status::status_bar(&state, cx)", |_, cx| {
                    status_bar(&state(json!({ "state": { "state": "running" }, "sessionInfo": { "title": "Fix the diff fold in dark mode" }, "usage": used(92_000, Some(0.31)) }), json!({}), &[]), cx)
                })
                .wide(),
                specimen("StatusBar · needs action, context nearly full", "status::status_bar(&state, cx)", |_, cx| {
                    status_bar(&state(json!({ "state": { "state": "requires_action" }, "sessionInfo": { "title": "Review the launchd plist" }, "usage": used(171_000, Some(2.4)) }), json!({}), &[]), cx)
                })
                .note("The meter turns to the attention colour at 80%.")
                .wide(),
                specimen("StatusBar · new session, no usage", "status::status_bar(&state, cx)", |_, cx| status_bar(&state(json!({ "state": { "state": "idle" } }), json!({}), &[]), cx)).wide(),
                specimen("Usage · context only (Pi)", "status::usage(&state, cx)", |_, cx| div().flex().child(usage(&state(json!({ "usage": used(48_000, None) }), json!({}), &[]), cx)))
                    .note("Pi gives no cost."),
                specimen("Activity · tasks and background work", "status::activity(key, &state, cx)", |_, cx| {
                    let state = state(
                        json!({ "state": { "state": "idle", "background": [{ "terminalId": "running" }, { "sessionId": "x" }] } }),
                        json!({ "plan": { "p": { "entries": plan_entries() } }, "terminal": { "running": terminals()["running"] }, "subagent": { "x": { "title": "Find every place that sorts sessions" } } }),
                        &[("plan", "p")],
                    );
                    div().children(activity("cat/activity", &state, cx))
                })
                .wide(),
                specimen("Activity · tasks open", "status::activity(key, &state, cx)", |_, cx| {
                    let state = state(json!({}), json!({ "plan": { "p": { "entries": plan_entries() } } }), &[("plan", "p")]);
                    div().children(activity("cat/activity-open", &state, cx))
                })
                .wide(),
                specimen("OpenElsewhere · with fork", "status::open_elsewhere(key, &holder, Some(fork), cx)", |_, cx| {
                    open_elsewhere("cat/elsewhere", &OpenElsewhere { pid: Some(48213), status: Some("busy".into()) }, Some(Rc::new(|_, _| Task::ready(Err("the agent cannot fork".to_owned())))), cx)
                })
                .note("Press it: this one fails, and says why.")
                .wide(),
                specimen("OpenElsewhere · without fork", "status::open_elsewhere(key, &holder, None, cx)", |_, cx| {
                    open_elsewhere("cat/elsewhere2", &OpenElsewhere { pid: Some(48213), status: None }, None, cx)
                })
                .wide(),
            ],
        },
        Section {
            id: "input",
            title: "Settings and composer",
            intro: "The settings come from what the agent reports: a menu per choice, a box to tick per on/off setting.",
            specimens: vec![
                specimen("SettingsBar", "settings_bar(key, &Setting::all(config_options), Some(change), cx)", |_, cx| {
                    div().children(settings_bar("cat/settings", &Setting::all(Some(&config_options())), Some(Rc::new(|_, _, _, _| Task::ready(Err("the agent refused".to_owned())))), cx))
                })
                .note("Change one: this agent refuses, so the bar says why and the control goes back.")
                .wide(),
                specimen("SettingsBar · read only", "settings_bar(key, &settings, None, cx)", |_, cx| {
                    div().children(settings_bar("cat/settings2", &Setting::all(Some(&config_options())), None, cx))
                })
                .note("Without change, or while a change is being confirmed.")
                .wide(),
                specimen("Composer · idle", "cx.new(|cx| Composer::new(send, window, cx))", move |_, _| idle.clone()).wide(),
                specimen("Composer · running", "composer.set_running(true, cx)", move |_, _| running.clone()).note("The placeholder changes and Stop appears.").wide(),
                specimen("Composer · can attach", "composer.set_accepts(Accepts { image: true, files: true }, cx)", move |_, _| attaching.clone()).wide(),
                specimen("Composer · command menu", "composer.set_commands(Command::all(available), cx)", move |_, _| commanding.clone())
                    .note("After typing \"/\". The chosen row is highlighted; arrows move it.")
                    .wide(),
                specimen("Composer · mention menu", "composer.set_mentions(Some(find), cx)", move |_, _| mentioning.clone())
                    .note("After typing \"@\", with the files the app supplies.")
                    .wide(),
            ],
        },
        Section {
            id: "notices",
            title: "Agent and connection",
            intro: "Notices for the app's own chrome. Both draw nothing when all is well.",
            specimens: vec![
                specimen("AgentStatus", "status::agent_status(&agents, Some(retry), cx)", |_, cx| {
                    let agents = [
                        agent("claude", "Claude Code", "starting", None),
                        agent("codex", "Codex", "needs_login", None),
                        agent("pi", "Pi", "not_installed", Some("pi was not found on PATH")),
                        agent("gemini", "Gemini", "error", Some("exited with code 1")),
                    ];
                    agent_status(&agents, Some(Rc::new(|_, _| {})), cx)
                })
                .note("Three tones: wait (muted), act (attention colour), bad (error colour).")
                .wide(),
                specimen("ConnectionStatus · lost", "status::connection_status(false, cx)", |_, cx| connection_status(false, cx)).wide(),
            ],
        },
        Section {
            id: "sessions",
            title: "Session list",
            intro: "Grouped by folder. A row says what needs the user first; rename and delete appear on the row under the pointer.",
            specimens: vec![
                specimen("SessionList", "cx.new(|_| SessionList::new(sessions))", move |_, _| list.clone())
                    .note("needs you · running · open elsewhere · from <agent> (started outside the app) · untitled. Deleting fails here, to show how.")
                    .sized(340.0, Some(560.0)),
                specimen("SessionList · empty", "cx.new(|_| SessionList::new(Vec::new()))", move |_, _| empty_list.clone()).sized(340.0, Some(120.0)),
                specimen("SessionRow", "session_row(&session, now, cx)", |_, cx| {
                    let rows: Vec<_> = sessions().iter().take(4).map(|session| div().flex().w_full().child(session_row(session, now(), cx))).collect();
                    div().flex().flex_col().gap(px(8.0)).children(rows)
                })
                .note("One row, for a list of your own.")
                .sized(340.0, None),
            ],
        },
        Section {
            id: "chat",
            title: "Assembled: ChatView",
            intro: "Everything above, put together by ChatView: status, timeline, activity, settings and composer.",
            specimens: vec![specimen("ChatView", "cx.new(|cx| ChatView::new(conversation, actions, window, cx))", move |_, _| chat.clone()).sized(900.0, Some(760.0))],
        },
    ]
}
