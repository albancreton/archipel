//! Draws recorded sessions with the kit and presses what can be pressed, in GPUI's test
//! windows: no screen, no daemon.

use archipel_core::state::{Conversation, Request};
use archipel_core::transcript::Transcript;
use archipel_gpui::chat::inline_terminals;
use archipel_gpui::sessions::{SessionList, SessionListEvent, SessionSummary};
use archipel_gpui::settings::{Setting, SettingValue, settings_bar};
use archipel_gpui::{
    ChatActions, ChatView, Composer, ItemViews, TimelineRow, items, memory, timeline, timeline_rows,
};
use archipel_schema::rules::RuleSet;
use gpui::{
    Focusable as _, Modifiers, Task, TestAppContext, VisualTestContext, div, point, prelude::*, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A recorded session, rebuilt by applying its messages.
fn recorded(name: &str) -> Conversation {
    let text = std::fs::read_to_string(root().join(format!("testdata/transcripts/{name}.ndjson")))
        .expect(name);
    Transcript::parse(&text)
        .expect(name)
        .conversation(&RuleSet::pinned())
        .expect(name)
}

/// The same, with its requests open again.
fn waiting(name: &str) -> Conversation {
    let mut conversation = recorded(name);
    for state in conversation.sessions.values_mut() {
        for request in state.requests.values_mut() {
            request.outcome = None;
        }
    }
    conversation
}

/// A view that draws what a test gives it: the kit's parts are drawn inside a view.
struct Host(archipel_gpui::parts::Draw);

impl gpui::Render for Host {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        div().size_full().child((self.0)(window, cx))
    }
}

/// A window that draws `draw`, again after each change.
fn host<E: IntoElement>(
    cx: &mut TestAppContext,
    draw: impl Fn(&mut gpui::Window, &mut gpui::App) -> E + 'static,
) -> &mut VisualTestContext {
    let (_, cx) = cx.add_window_view(|_, _| {
        Host(Box::new(move |window, cx| {
            draw(window, cx).into_any_element()
        }))
    });
    cx.run_until_parked();
    cx
}

/// Presses the middle of the element drawn under `selector`.
fn press(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing is drawn as {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn every_recorded_session_is_drawn(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let folder = root().join("testdata/transcripts");
    let mut drawn = 0;
    for harness in ["claude-v2", "pi-v2"] {
        let mut names: Vec<_> = std::fs::read_dir(folder.join(harness))
            .expect(harness)
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        names.sort();
        for name in names.iter().filter_map(|name| name.strip_suffix(".ndjson")) {
            let conversation = recorded(&format!("{harness}/{name}"));
            let Some((id, state)) = conversation.root() else {
                panic!("{name}: no session")
            };
            assert!(!state.timeline.is_empty(), "{name}: nothing to draw");
            let (id, conversation) = (id.clone(), conversation.clone());
            host(cx, move |window, cx| {
                let state = &conversation.sessions[&id];
                div().w(px(760.0)).child(timeline(
                    &conversation,
                    &id,
                    state,
                    &ChatActions::default(),
                    &ItemViews::new(),
                    window,
                    cx,
                ))
            });
            drawn += 1;
        }
    }
    assert!(drawn >= 17, "{drawn} sessions were drawn");
}

#[gpui::test]
fn a_fold_opens_when_pressed(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let item =
        json!({ "content": [{ "type": "text", "text": "Let me look at **sessions.rs** first." }] });
    let cx = host(cx, move |window, cx| {
        div()
            .w(px(600.0))
            .child(items::thought("fold-test", &item, window, cx))
    });
    let closed = cx.debug_bounds("fold-test").expect("the fold is drawn");
    assert!(!cx.update(|_, cx| memory::flag(cx, "fold-test", false)));
    press(cx, "fold-test");
    assert!(
        cx.update(|_, cx| memory::flag(cx, "fold-test", false)),
        "pressing the summary opens the fold"
    );
    assert_eq!(
        cx.debug_bounds("fold-test").map(|bounds| bounds.origin),
        Some(closed.origin)
    );
}

/// The rows of the conversation's first session, each as its kinds: one for an entry, several
/// for a group.
fn row_kinds(conversation: &Conversation) -> Vec<Vec<String>> {
    let (_, state) = conversation.root().expect("a session");
    timeline_rows(state, &inline_terminals(state))
        .into_iter()
        .map(|row| match row {
            TimelineRow::Item(entry) => vec![entry.target],
            TimelineRow::Group(entries) => entries.into_iter().map(|entry| entry.target).collect(),
        })
        .collect()
}

/// What the line of a group says: how many calls, and how many failed.
type Title = (String, Option<String>);

/// The groups among the rows, and what the line of each says.
fn groups(conversation: &Conversation) -> Vec<(Vec<String>, Title)> {
    let (_, state) = conversation.root().expect("a session");
    timeline_rows(state, &inline_terminals(state))
        .into_iter()
        .filter_map(|row| match row {
            TimelineRow::Group(entries) => {
                let calls: Vec<&Value> = entries
                    .iter()
                    .filter(|entry| entry.target == "toolCall")
                    .filter_map(|entry| state.item(entry))
                    .collect();
                let title = items::tool_group_title(&calls);
                Some((
                    entries.into_iter().map(|entry| entry.target).collect(),
                    title,
                ))
            }
            TimelineRow::Item(_) => None,
        })
        .collect()
}

/// A window that draws the whole timeline of the conversation's first session.
fn drawn<'a>(cx: &'a mut TestAppContext, conversation: &Conversation) -> &'a mut VisualTestContext {
    let id = conversation.root().expect("a session").0.clone();
    let conversation = conversation.clone();
    host(cx, move |window, cx| {
        let state = &conversation.sessions[&id];
        div().w(px(760.0)).child(timeline(
            &conversation,
            &id,
            state,
            &ChatActions::default(),
            &ItemViews::new(),
            window,
            cx,
        ))
    })
}

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

#[gpui::test]
fn finished_tool_calls_fold_into_one_group_and_a_call_shows_its_output_only_while_it_runs(
    cx: &mut TestAppContext,
) {
    cx.update(archipel_gpui::init);
    // Recorded: Read, a command, a thought, Edit (with a diff), its approval, then the answer.
    let mut conversation = recorded("claude-v2/files");
    let session = conversation.root().expect("a session").0.clone();
    let strs = |kinds: &[&str]| {
        kinds
            .iter()
            .map(|kind| (*kind).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        groups(&conversation),
        [(
            strs(&[
                "toolCall",
                "toolCall",
                "agentThought",
                "toolCall",
                "request"
            ]),
            ("3 tool calls".to_owned(), None)
        )],
        "the thought between two calls and the answered approval after one are in the group"
    );
    let edit = {
        let (_, state) = conversation.root().expect("a session");
        state.collections["toolCall"]
            .iter()
            .find(|(_, tool)| tool["kind"] == "edit")
            .map(|(id, _)| id.clone())
            .expect("an edit")
    };
    let output = leak(format!("{session}/toolCall/{edit}/output"));
    {
        let window = drawn(cx, &conversation);
        assert!(
            window.debug_bounds(output).is_none(),
            "what the calls produced is folded away"
        );
    }

    // The edit is still going: it leaves the group and shows its diff.
    let set = |conversation: &mut Conversation, status: &str, turn: &str| {
        let state = conversation.session_mut(&session);
        state
            .collections
            .get_mut("toolCall")
            .and_then(|tools| tools.get_mut(&edit))
            .expect("the edit")["status"] = json!(status);
        state.values.get_mut("state").expect("a turn")["state"] = json!(turn);
    };
    set(&mut conversation, "in_progress", "running");
    assert_eq!(
        row_kinds(&conversation)[1..],
        [
            strs(&["toolCall", "toolCall"]),
            strs(&["agentThought"]),
            strs(&["toolCall"]),
            strs(&["request"]),
            strs(&["agentMessage"]),
        ],
        "the thought after the group's last call is on its own again"
    );
    {
        let window = drawn(cx, &conversation);
        assert!(
            window.debug_bounds(output).is_some(),
            "a running call shows its diff"
        );
    }

    // A call left as going by a turn that was cut short is over, and one that failed is counted.
    set(&mut conversation, "in_progress", "idle");
    assert_eq!(groups(&conversation)[0].1.0, "3 tool calls");
    set(&mut conversation, "failed", "idle");
    assert_eq!(
        groups(&conversation)[0].1,
        ("3 tool calls".to_owned(), Some(" · 1 failed".to_owned()))
    );
}

#[gpui::test]
fn one_tool_call_alone_is_not_a_group(_: &mut TestAppContext) {
    assert!(groups(&recorded("claude-v2/command_output")).is_empty());
}

#[gpui::test]
fn an_answered_approval_goes_into_the_group_of_the_call_it_follows(_: &mut TestAppContext) {
    let conversation = recorded("claude-v2/plan");
    let found = groups(&conversation);
    let calls = |kinds: &[String]| kinds.iter().filter(|kind| *kind == "toolCall").count();
    let approvals = |kinds: &[String]| kinds.iter().filter(|kind| *kind == "request").count();
    let (first, last) = (
        found.first().expect("a group"),
        found.last().expect("a group"),
    );
    assert_eq!((calls(&first.0), approvals(&first.0)), (4, 1));
    assert_eq!((calls(&last.0), approvals(&last.0)), (2, 2));
    assert_eq!(found.len(), 2);
    // The first group's approval is the plan's.
    let (_, state) = conversation.root().expect("a session");
    let rows = timeline_rows(state, &inline_terminals(state));
    let asked: Vec<&str> = rows
        .iter()
        .find_map(|row| match row {
            TimelineRow::Group(entries) => Some(entries),
            TimelineRow::Item(_) => None,
        })
        .expect("a group")
        .iter()
        .filter(|entry| entry.target == "request")
        .filter_map(|entry| state.requests.get(&entry.id)?.params["title"].as_str())
        .collect();
    assert_eq!(asked, ["Ready to code?"]);
}

#[gpui::test]
fn a_tool_call_and_a_group_open_and_close_when_pressed(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let call = |status: &str, content: Value| json!({ "kind": "read", "title": "a.rs", "status": status, "content": content });
    let said =
        json!([{ "type": "content", "content": { "type": "text", "text": "fn main() {}" } }]);
    let (done, going, bare) = (
        call("completed", said.clone()),
        call("in_progress", said),
        call("completed", json!([])),
    );
    {
        let cx = host(cx, move |_, cx| {
            div()
                .w(px(600.0))
                .child(items::tool_call("done", &done, false, |_| None, cx))
                .child(items::tool_call("going", &going, true, |_| None, cx))
                .child(items::tool_call("bare", &bare, false, |_| None, cx))
        });
        // GPUI keeps the bounds of what it drew before, so what closed is seen by what moved up.
        let top = |cx: &mut VisualTestContext, selector: &'static str| {
            cx.debug_bounds(selector).expect(selector).origin.y
        };
        assert!(cx.debug_bounds("done/output").is_none(), "over: closed");
        assert!(cx.debug_bounds("going/output").is_some(), "running: open");
        let (going, bare) = (top(cx, "going"), top(cx, "bare"));
        press(cx, "done");
        assert!(cx.debug_bounds("done/output").is_some());
        assert!(
            top(cx, "going") > going,
            "the output pushes the next call down"
        );
        press(cx, "done");
        assert_eq!(top(cx, "going"), going, "pressed again, it is closed");
        // What the user chose stays: a call they closed does not open again while it runs.
        press(cx, "going");
        assert!(top(cx, "bare") < bare);
        assert!(!cx.update(|_, cx| memory::flag(cx, "going", true)));
        // With nothing to show, the line is not a control.
        press(cx, "bare");
        assert!(cx.update(|_, cx| !memory::flag(cx, "bare", false)));
        assert!(cx.debug_bounds("bare/output").is_none());
    }

    // A group opens to its calls, each still closed, and each opens on its own.
    let conversation = recorded("claude-v2/files");
    let (session, state) = conversation.root().expect("a session");
    let first = state
        .timeline
        .iter()
        .find(|entry| entry.target == "toolCall")
        .expect("a call")
        .id
        .clone();
    let group = leak(format!("{session}/toolGroup/{first}"));
    let member = leak(format!("{session}/toolCall/{first}"));
    let output = leak(format!("{session}/toolCall/{first}/output"));
    let cx = drawn(cx, &conversation);
    assert!(cx.debug_bounds(group).is_some(), "the group is drawn");
    assert!(
        cx.debug_bounds(member).is_none(),
        "closed: its calls are not"
    );
    press(cx, group);
    assert!(cx.debug_bounds(member).is_some());
    assert!(cx.debug_bounds(output).is_none());
    press(cx, member);
    assert!(cx.debug_bounds(output).is_some());
    let open = cx.debug_bounds(group).expect("the group").origin.y;
    press(cx, group);
    assert!(!cx.update(|_, cx| memory::flag(cx, group, true)));
    // The timeline's rows are under one another from the top, so the group does not move; what
    // was under it does.
    assert_eq!(
        cx.debug_bounds(group).map(|bounds| bounds.origin.y),
        Some(open)
    );
}

#[gpui::test]
fn an_approval_gives_the_option_that_was_pressed(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let conversation = waiting("claude-v2/approval_command");
    let (request_id, request): (String, Request) = conversation
        .root()
        .and_then(|(_, state)| state.requests.first())
        .map(|(id, request)| (id.clone(), request.clone()))
        .expect("a request");
    let options = request.params["options"]
        .as_array()
        .expect("options")
        .clone();
    assert!(options.len() > 1);
    let chosen: Rc<RefCell<Vec<String>>> = Rc::default();
    let heard = chosen.clone();
    let cx = host(cx, move |_, cx| {
        let heard = heard.clone();
        div().w(px(700.0)).child(items::approval(
            "approval-test",
            &request,
            Some(Rc::new(move |option, _, _| {
                heard.borrow_mut().push(option.to_owned())
            })),
            cx,
        ))
    });
    press(cx, "approval-test/option1");
    assert_eq!(
        *chosen.borrow(),
        [options[1]["optionId"].as_str().expect("an id").to_owned()],
        "request {request_id}"
    );
}

#[gpui::test]
fn a_question_is_answered_with_what_was_typed(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let request = Request {
        method: "session/elicitation".to_owned(),
        params: json!({ "message": "Which?", "requestedSchema": { "properties": {
            "kit": { "title": "Kit", "oneOf": [{ "const": "react", "title": "React" }, { "const": "swift", "title": "SwiftUI" }] },
            "note": { "title": "Anything else?" },
        } } }),
        outcome: None,
    };
    let answers: Rc<RefCell<Vec<Option<Value>>>> = Rc::default();
    let heard = answers.clone();
    let cx = host(cx, move |window, cx| {
        let heard = heard.clone();
        div().w(px(700.0)).h(px(500.0)).child(items::question(
            "q",
            &request,
            Some(Rc::new(move |content, _, _| {
                heard.borrow_mut().push(content)
            })),
            window,
            cx,
        ))
    });
    press(cx, "q/kit/toggle");
    press(cx, "q/kit/swift");
    // The text field takes what is typed once it has the focus.
    let field = cx.debug_bounds("q/answer").expect("the form is drawn");
    cx.simulate_click(
        point(field.origin.x + px(40.0), field.origin.y - px(30.0)),
        Modifiers::none(),
    );
    cx.simulate_input("then React");
    cx.run_until_parked();
    press(cx, "q/answer");
    press(cx, "q/skip");
    assert_eq!(
        *answers.borrow(),
        [Some(json!({ "kit": "swift", "note": "then React" })), None]
    );
}

#[gpui::test]
fn the_composer_sends_and_gives_back_what_was_refused(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let sent: Rc<RefCell<Vec<Vec<Value>>>> = Rc::default();
    let refuse = Rc::new(RefCell::new(false));
    let (heard, refusing) = (sent.clone(), refuse.clone());
    let (composer, cx) = cx.add_window_view(|window, cx| {
        Composer::new(
            Rc::new(move |content, _, _| {
                heard.borrow_mut().push(content);
                Task::ready(if *refusing.borrow() {
                    Err("the log cannot be written".to_owned())
                } else {
                    Ok(())
                })
            }),
            window,
            cx,
        )
    });
    composer.update_in(cx, |composer, window, cx| {
        composer.set_commands(
            archipel_gpui::Command::all(Some(
                &json!([{ "name": "compact" }, { "name": "review" }]),
            )),
            cx,
        );
        window.focus(&composer.field().focus_handle(cx));
    });
    cx.run_until_parked();

    cx.simulate_input("hello");
    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("there");
    assert_eq!(
        composer.read_with(cx, |composer, cx| composer
            .field()
            .read(cx)
            .text()
            .to_owned()),
        "hello\nthere"
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        *sent.borrow(),
        [vec![json!({ "type": "text", "text": "hello\nthere" })]]
    );
    assert_eq!(
        composer.read_with(cx, |composer, cx| composer
            .field()
            .read(cx)
            .text()
            .to_owned()),
        ""
    );

    // The command menu: the arrows move in it, Tab takes the chosen one.
    cx.simulate_input("/");
    cx.simulate_keystrokes("down tab");
    assert_eq!(
        composer.read_with(cx, |composer, cx| composer
            .field()
            .read(cx)
            .text()
            .to_owned()),
        "/review "
    );

    *refuse.borrow_mut() = true;
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(sent.borrow().len(), 2);
    assert_eq!(
        composer.read_with(cx, |composer, cx| composer
            .field()
            .read(cx)
            .text()
            .to_owned()),
        "/review ",
        "a refused message goes back into the field"
    );
}

#[gpui::test]
fn a_chat_draws_a_session_and_follows_it(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let full = waiting("claude-v2/approval_command");
    let approved: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
    let heard = approved.clone();
    let actions = ChatActions {
        approve: Some(Rc::new(move |request, option, _, _| {
            heard
                .borrow_mut()
                .push((request.to_owned(), option.to_owned()))
        })),
        prompt: Some(Rc::new(|_, _, _| Task::ready(Ok(())))),
        ..ChatActions::default()
    };
    // Starts with nothing, as a session does, then gets its state.
    let (chat, cx) = cx
        .add_window_view(|window, cx| ChatView::new(Conversation::default(), actions, window, cx));
    cx.run_until_parked();
    assert!(chat.read_with(cx, |chat, _| chat.session_id().is_none()));
    assert!(cx.debug_bounds("send").is_none(), "no session, no composer");
    // A session that was just made holds nothing, and can be written to.
    chat.update(cx, |chat, cx| chat.set_session(Some("new".to_owned()), cx));
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("send").is_some(),
        "a new session has its composer"
    );
    chat.update(cx, |chat, cx| chat.set_session(None, cx));
    chat.update(cx, |chat, cx| chat.set_conversation(full.clone(), cx));
    cx.run_until_parked();
    let (request_id, _) = full
        .root()
        .and_then(|(_, state)| state.requests.first())
        .expect("a request");
    let session = full.root().map(|(id, _)| id.clone()).expect("a session");
    let selector: &'static str =
        Box::leak(format!("{session}/request/{request_id}/option0").into_boxed_str());
    press(cx, selector);
    assert_eq!(approved.borrow().len(), 1);
    assert_eq!(&approved.borrow()[0].0, request_id);
}

#[gpui::test]
fn a_refused_setting_goes_back_and_says_why(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let options = json!([
        { "configId": "model", "name": "Model", "type": "select", "currentValue": "sonnet", "options": [{ "value": "sonnet", "name": "Sonnet" }, { "value": "opus", "name": "Opus" }] },
        { "configId": "fast", "name": "Fast mode", "type": "boolean", "currentValue": false },
    ]);
    let asked: Rc<RefCell<Vec<(String, SettingValue)>>> = Rc::default();
    let heard = asked.clone();
    let cx = host(cx, move |_, cx| {
        let heard = heard.clone();
        let change = Rc::new(
            move |id: &str, value: SettingValue, _: &mut gpui::Window, _: &mut gpui::App| {
                heard.borrow_mut().push((id.to_owned(), value));
                Task::ready(Err("not on this plan".to_owned()))
            },
        );
        div().w(px(700.0)).h(px(400.0)).children(settings_bar(
            "s",
            &Setting::all(Some(&options)),
            Some(change),
            cx,
        ))
    });
    press(cx, "s/model/toggle");
    press(cx, "s/model/opus");
    press(cx, "s/fast");
    assert_eq!(
        asked.borrow()[0],
        ("model".to_owned(), SettingValue::Choice("opus".to_owned()))
    );
    assert_eq!(
        cx.update(|_, cx| memory::text(cx, "s/problem")),
        Some("Fast mode: not on this plan".to_owned())
    );
    assert_eq!(
        cx.update(|_, cx| memory::text(cx, "s/asked")),
        None,
        "nothing is left waiting"
    );
}

#[gpui::test]
fn a_session_list_opens_what_is_pressed(cx: &mut TestAppContext) {
    cx.update(archipel_gpui::init);
    let sessions: Vec<_> = [
        json!({ "sessionId": "s1", "agentId": "claude", "cwd": "/work/app", "title": "Fix the login", "state": "requires_action" }),
        json!({ "sessionId": "s2", "agentId": "claude", "cwd": "/work/site", "title": "Docs" }),
    ]
    .iter()
    .filter_map(SessionSummary::of)
    .collect();
    let (list, cx) = cx.add_window_view(|_, _| SessionList::new(sessions));
    let opened: Rc<RefCell<Vec<String>>> = Rc::default();
    let heard = opened.clone();
    let _watch = cx.update(|_, cx| {
        cx.subscribe(&list, move |_, event: &SessionListEvent, _| {
            let SessionListEvent::Opened(id) = event;
            heard.borrow_mut().push(id.clone());
        })
    });
    cx.run_until_parked();
    press(cx, "s2/row");
    assert_eq!(*opened.borrow(), ["s2"]);
    assert_eq!(
        list.read_with(cx, |list, _| list.selected().map(str::to_owned)),
        Some("s2".to_owned())
    );
}
