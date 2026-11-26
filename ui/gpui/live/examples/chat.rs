//! A chat on one session of a running daemon, in a window: the smallest GPUI app on the kit.
//!
//!   cargo run -p archipel-gpui-live --example chat -- [--home DIR] [--cwd DIR] [--session ID] [--theme minitel] [--light]
//!
//! (from `ui/gpui`). Without `--session` it starts a new one in `--cwd` (the default is the
//! current folder). Where the daemon is can also be given as `--socket`, `--ws` with `--token`,
//! or `--via COMMAND`, as for the `archipel` command.
//!
//! To try it with no model and no account, start a daemon with the fake agent first:
//!
//!   target/debug/archipeld --home /tmp/try --fake testdata/transcripts/claude-v2/simple.ndjson &

use archipel_client::{Endpoint, option, switch};
use archipel_gpui::parts::{NoticeTone, notice};
use archipel_gpui::{ChatView, Theme, theme};
use archipel_gpui_live::{Client, Link, Session, live_chat};
use gpui::{
    App, Application, Bounds, Context, Entity, IntoElement, Render, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};

struct Demo {
    client: Entity<Client>,
    /// What to open once connected: a session's id, or a folder to start one in.
    wanted: Option<Result<String, String>>,
    chat: Option<Entity<ChatView>>,
    problem: Option<String>,
}

impl Demo {
    fn show(&mut self, session: &Entity<Session>, window: &mut Window, cx: &mut Context<Self>) {
        // A fork is shown in place of the session it copies.
        let demo = cx.weak_entity();
        let on_fork = std::rc::Rc::new(
            move |copy: Entity<Session>, window: &mut Window, cx: &mut App| {
                demo.update(cx, |demo, cx| demo.show(&copy, window, cx))
                    .ok();
            },
        );
        self.chat = Some(live_chat(session, Some(on_fork), window, cx));
        cx.notify();
    }
}

impl Render for Demo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        let link = self.client.read(cx).link.clone();
        // The first time the daemon answers, open what was asked for.
        if link == Link::Connected
            && let Some(wanted) = self.wanted.take()
        {
            match wanted {
                Ok(id) => {
                    let session = self.client.update(cx, |client, cx| client.session(&id, cx));
                    self.show(&session, window, cx);
                }
                Err(cwd) => {
                    let created = self
                        .client
                        .update(cx, |client, cx| client.new_session(&cwd, None, cx));
                    cx.spawn_in(window, async move |demo, cx| {
                        let created = created.await;
                        demo.update_in(cx, |demo, window, cx| match created {
                            Ok(session) => demo.show(&session, window, cx),
                            Err(refusal) => {
                                demo.problem = Some(format!("No session: {refusal}"));
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                    .detach();
                }
            }
        }
        let status = match (&self.problem, &link) {
            (Some(problem), _) => Some((problem.clone(), NoticeTone::Bad)),
            (None, Link::Retrying(why)) => Some((
                format!("No connection to the daemon ({why}). Trying again…"),
                NoticeTone::Wait,
            )),
            (None, Link::Connecting) => Some(("Connecting…".to_owned(), NoticeTone::Wait)),
            (None, Link::Connected) => None,
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.colors.background)
            .text_color(theme.colors.text)
            .when_some(status, |column, (text, tone)| {
                column.child(
                    div()
                        .p(px(12.0))
                        .child(notice("status", &text, tone, None, cx)),
                )
            })
            .child(div().flex_1().min_h_0().children(self.chat.clone()))
    }
}

fn main() {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    let dark = !switch(&mut arguments, "--light");
    let minitel = option(&mut arguments, "--theme").as_deref() == Some("minitel");
    let wanted = match option(&mut arguments, "--session") {
        Some(id) => Ok(id),
        None => Err(option(&mut arguments, "--cwd").unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string()
        })),
    };
    let endpoint = match Endpoint::from_args(&mut arguments) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("chat: {error}");
            std::process::exit(2);
        }
    };
    Application::new().run(move |cx: &mut App| {
        archipel_gpui::init(cx);
        archipel_gpui::set_theme(
            cx,
            if minitel {
                archipel_gpui_minitel::minitel(dark)
            } else {
                Theme::standard(dark)
            },
        );
        let client = Client::connect(endpoint, "gpui-chat", cx);
        let bounds = Bounds::centered(None, size(px(860.0), px(900.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| {
            cx.new(|cx| {
                cx.observe(&client, |_, _, cx| cx.notify()).detach();
                Demo {
                    client,
                    wanted: Some(wanted),
                    chat: None,
                    problem: None,
                }
            })
        })
        .expect("the window opens");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
