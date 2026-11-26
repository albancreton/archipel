//! Drives a session end to end through the kit's own controls, against a daemon whose agent
//! replays a recorded session: no model, no account. Needs `cargo build` at the repository's
//! root first, for `target/debug/archipeld`.

use archipel_client::Endpoint;
use archipel_gpui::SettingValue;
use archipel_gpui_live::{Client, live_chat};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Daemon(Child, PathBuf);

impl Drop for Daemon {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
        std::fs::remove_dir_all(&self.1).ok();
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Starts a daemon with the fake agent on a recording, in a home of its own.
fn daemon(recording: &str) -> Daemon {
    let home =
        std::env::temp_dir().join(format!("archipel-gpui-{}-{recording}", std::process::id()));
    std::fs::remove_dir_all(&home).ok();
    std::fs::create_dir_all(&home).expect("a home for the daemon");
    let program = root().join("target/debug/archipeld");
    assert!(
        program.exists(),
        "run `cargo build` at the repository's root first"
    );
    let child = Command::new(program)
        .arg("--home")
        .arg(&home)
        .arg("--fake")
        .arg(root().join(format!("testdata/transcripts/claude-v2/{recording}.ndjson")))
        .args(["--fake-speed", "0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the daemon starts");
    Daemon(child, home)
}

/// Waits, in real time, for something that depends on the daemon.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..500 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("waited ten seconds for: {what}");
}

/// Waits for a task of the app, letting the app run meanwhile.
fn wait<T: 'static>(cx: &mut VisualTestContext, what: &str, task: gpui::Task<T>) -> T {
    let result = std::rc::Rc::new(std::cell::RefCell::new(None));
    let kept = result.clone();
    cx.spawn(async move |_| {
        let value = task.await;
        *kept.borrow_mut() = Some(value);
    })
    .detach();
    until(cx, what, |_| result.borrow().is_some());
    result.borrow_mut().take().expect("the task finished")
}

#[gpui::test]
fn a_session_is_driven_through_the_chat(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(archipel_gpui::init);
    let daemon = daemon("approval_command");
    let client = cx.update(|cx| {
        Client::connect(
            Endpoint::Local(daemon.1.join("daemon.sock")),
            "gpui-test",
            cx,
        )
    });
    let cx = cx.add_empty_window();
    until(cx, "the connection", |cx| {
        client.read_with(cx, |client, _| client.connected())
    });

    // The recording is: a new session, the mode, one prompt, one approval.
    let created = client.update(cx, |client, cx| client.new_session("/tmp", None, cx));
    let session = wait(cx, "a new session", created).expect("a session");
    let id = session.read_with(cx, |session, _| session.id.clone());
    let set = session.update(cx, |session, cx| {
        session.set_config_option("mode", SettingValue::Choice("default".to_owned()), cx)
    });
    wait(cx, "the mode", set).expect("the mode is set");

    let window = cx.update(|window, _| window.window_handle());
    let chat = cx.update(|window, cx| live_chat(&session, None, window, cx));
    window
        .update(cx, |_, window, cx| {
            window.replace_root(cx, |_, _| ChatHost(chat.clone()))
        })
        .expect("the chat is shown");
    cx.run_until_parked();

    // The message is typed and sent as a user would.
    chat.update_in(cx, |chat, window, cx| {
        let field = chat
            .composer()
            .expect("a composer")
            .read(cx)
            .field()
            .clone();
        window.focus(&gpui::Focusable::focus_handle(&field, cx));
    });
    cx.simulate_input("list the files");
    cx.simulate_keystrokes("enter");

    until(cx, "the approval", |cx| {
        session.read_with(cx, |session, _| {
            session.turn_state() == Some("requires_action")
        })
    });
    let request = session.read_with(cx, |session, _| {
        let state = &session.conversation().sessions[&id];
        state
            .pending_requests()
            .next()
            .map(|(request, _)| request.clone())
            .expect("a request waits")
    });
    let selector: &'static str =
        Box::leak(format!("{id}/request/{request}/option0").into_boxed_str());
    until(cx, "the approval's buttons", |cx| {
        cx.debug_bounds(selector).is_some()
    });
    let button = cx.debug_bounds(selector).expect("the first option");
    cx.simulate_click(button.center(), Modifiers::none());

    until(cx, "the end of the turn", |cx| {
        session.read_with(cx, |session, _| session.turn_state() == Some("idle"))
    });
    session.read_with(cx, |session, _| {
        let state = &session.conversation().sessions[&id];
        let targets: Vec<&str> = state
            .timeline
            .iter()
            .map(|entry| entry.target.as_str())
            .collect();
        assert!(
            targets.contains(&"userMessage")
                && targets.contains(&"toolCall")
                && targets.contains(&"agentMessage"),
            "{targets:?}"
        );
        assert!(
            state.requests[&request].outcome.is_some(),
            "the request shows its answer"
        );
    });
    client.read_with(cx, |client, _| {
        assert!(
            client
                .summaries()
                .iter()
                .any(|summary| summary.session_id == id),
            "the session is in the list"
        )
    });
}

struct ChatHost(gpui::Entity<archipel_gpui::ChatView>);

impl gpui::Render for ChatHost {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        self.0.clone()
    }
}
