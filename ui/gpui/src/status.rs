//! Around the timeline: the status bar, usage, what goes on beside the conversation, and the
//! notices of the app's chrome (agents that cannot be used, a lost connection, a session open
//! elsewhere).

use crate::items::{plan_entry, shared, text};
use crate::memory;
use crate::parts::{
    Handler, NoticeTone, badge, button, disabled, fold, meter, notice, problem, tip,
};
use crate::theme::theme;
use archipel_core::state::SessionState;
use gpui::{App, Div, FontWeight, SharedString, Task, Window, div, prelude::*, px};
use serde_json::Value;
use std::rc::Rc;

/// What an action that can fail gives back: nothing, or why it failed, in words for the user.
pub type Outcome = Task<Result<(), String>>;

/// Continues on a copy of the session. The app shows the copy once the outcome arrives.
pub type Fork = Rc<dyn Fn(&mut Window, &mut App) -> Outcome>;

/// What to tell the user about an agent that cannot be used as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProblem {
    pub id: String,
    pub tone: NoticeTone,
    pub text: String,
}

impl AgentProblem {
    /// From one entry of the client's agents. `None` when the agent is ready, or was not started
    /// yet: then nothing is wrong as far as anyone knows.
    #[must_use]
    pub fn of(agent: &Value) -> Option<Self> {
        let name = [text(agent, "name"), text(agent, "agentId"), "The agent"]
            .into_iter()
            .find(|name| !name.is_empty())?;
        let detail = Some(text(agent, "detail"))
            .filter(|detail| !detail.is_empty())
            .map(|detail| format!(" ({detail})"))
            .unwrap_or_default();
        let (tone, text) = match text(agent, "readiness") {
            "starting" => (NoticeTone::Wait, format!("{name} is starting…")),
            "needs_login" => (
                NoticeTone::Act,
                format!(
                    "{name} is not signed in. Sign in with {name}'s own program, then try again."
                ),
            ),
            "not_installed" => (
                NoticeTone::Bad,
                format!("{name} is not installed on the daemon's machine{detail}."),
            ),
            "error" => (
                NoticeTone::Bad,
                format!("{name} could not be started{detail}."),
            ),
            _ => return None,
        };
        let id = Some(self::text(agent, "agentId"))
            .filter(|id| !id.is_empty())
            .unwrap_or(name)
            .to_owned();
        Some(Self { id, tone, text })
    }
}

/// Says which agents cannot be used and why: not signed in, not installed, failed to start, or
/// still starting. Draws nothing when all is well.
pub fn agent_status(agents: &[Value], retry: Option<Handler>, cx: &App) -> Div {
    let mut column = div().flex().flex_col().gap(px(6.0)).w_full();
    for found in agents.iter().filter_map(AgentProblem::of) {
        let action = retry
            .clone()
            .filter(|_| found.tone != NoticeTone::Wait)
            .map(|retry| ("Try again", retry));
        column = column.child(notice(
            format!("agent/{}", found.id),
            &found.text,
            found.tone,
            action,
            cx,
        ));
    }
    column
}

/// Says that the connection to the daemon is lost and being retried. Draws nothing while connected.
pub fn connection_status(connected: bool, cx: &App) -> Div {
    let column = div().w_full();
    if connected {
        return column;
    }
    column.child(notice(
        "connection",
        "Connection lost. Reconnecting… What you see may be out of date.",
        NoticeTone::Wait,
        None,
        cx,
    ))
}

/// What the session has used, as far as the agent says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UsageSummary {
    pub used: Option<u64>,
    pub size: Option<u64>,
    pub percent: Option<u32>,
    /// Such as `$0.0031`. `None` when the agent gives no cost.
    pub cost: Option<String>,
    pub turn_tokens: Option<u64>,
}

impl UsageSummary {
    #[must_use]
    pub fn of(state: &SessionState) -> Self {
        let usage = state.values.get("usage");
        let number = |field: &str| {
            usage
                .and_then(|usage| usage.get(field))
                .and_then(Value::as_f64)
        };
        let used = number("used").map(|used| used as u64);
        let size = number("size")
            .filter(|size| *size > 0.0)
            .map(|size| size as u64);
        let percent = used
            .zip(size)
            .map(|(used, size)| (used as f64 / size as f64 * 100.0).round() as u32);
        let cost = usage
            .and_then(|usage| usage.pointer("/cost/amount"))
            .and_then(Value::as_f64)
            .map(|amount| {
                let currency = usage
                    .and_then(|usage| usage.pointer("/cost/currency"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                // Small amounts keep more digits: a turn often costs less than a cent.
                let number = if amount < 0.01 {
                    format!("{amount:.4}")
                } else {
                    format!("{amount:.2}")
                };
                match currency {
                    "USD" => format!("${number}"),
                    "" => number,
                    other => format!("{number} {other}"),
                }
            });
        let turn_tokens = state
            .values
            .get("state")
            .and_then(|turn| turn.pointer("/usage/totalTokens"))
            .and_then(Value::as_f64)
            .map(|n| n as u64);
        Self {
            used,
            size,
            percent,
            cost,
            turn_tokens,
        }
    }
}

/// How full the context is, and what the session has cost when the agent says. The exact
/// tokens on hover. Nothing when neither is known.
pub fn usage(state: &SessionState, cx: &App) -> impl IntoElement + use<> {
    let theme = theme(cx);
    let summary = UsageSummary::of(state);
    let exact = [
        summary
            .used
            .zip(summary.size)
            .map(|(used, size)| format!("{used} of {size} tokens of context")),
        summary
            .turn_tokens
            .map(|tokens| format!("{tokens} tokens in the last turn")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    div()
        .id("usage")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .text_size(px(theme.text.meta))
        .text_color(theme.colors.secondary)
        .when_some(summary.percent, |row, percent| {
            row.child(meter(percent, cx))
                .child(SharedString::from(format!("{percent}% of context")))
        })
        .when_some(summary.cost, |row, cost| {
            row.child(SharedString::from(cost))
        })
        .when(!exact.is_empty(), |row| row.tooltip(tip(&exact)))
}

/// The session's title, turn state and context use.
pub fn status_bar(state: &SessionState, cx: &App) -> Div {
    let theme = theme(cx);
    let title = state
        .values
        .get("sessionInfo")
        .map(|info| text(info, "title"))
        .filter(|title| !title.is_empty());
    let turn = state
        .values
        .get("state")
        .map(|turn| text(turn, "state"))
        .filter(|turn| !turn.is_empty());
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(theme.text.title))
                .font_weight(FontWeight::SEMIBOLD)
                .child(shared(title.unwrap_or("New session"))),
        )
        .child(usage(state, cx))
        .when_some(turn, |row, turn| row.child(badge(turn, None, cx)))
}

/// One thing that still runs in the background after a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundWork {
    /// `command`, `tool` or `subagent`.
    pub kind: &'static str,
    pub id: String,
    pub label: String,
}

/// What the session says is running in the background, with each one's command or title.
#[must_use]
pub fn background_work(state: &SessionState) -> Vec<BackgroundWork> {
    let named = |target: &str, id: &str, field: &str| {
        state
            .collections
            .get(target)
            .and_then(|items| items.get(id))
            .map(|item| text(item, field))
            .filter(|name| !name.is_empty())
            .unwrap_or(id)
            .to_owned()
    };
    let works = state
        .values
        .get("state")
        .and_then(|turn| turn.get("background"))
        .and_then(Value::as_array);
    works
        .map(|works| {
            works
                .iter()
                .filter_map(|work| {
                    let id = |field: &str| work.get(field).and_then(Value::as_str);
                    if let Some(id) = id("terminalId") {
                        Some(BackgroundWork {
                            kind: "command",
                            id: id.to_owned(),
                            label: named("terminal", id, "command"),
                        })
                    } else if let Some(id) = id("toolCallId") {
                        Some(BackgroundWork {
                            kind: "tool",
                            id: id.to_owned(),
                            label: named("toolCall", id, "title"),
                        })
                    } else {
                        id("sessionId").map(|id| BackgroundWork {
                            kind: "subagent",
                            id: id.to_owned(),
                            label: named("subagent", id, "title"),
                        })
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The newest todo list the agent keeps (a plan with entries): each entry's content and status.
#[must_use]
pub fn todos(state: &SessionState) -> Option<Vec<(String, String)>> {
    state
        .timeline
        .iter()
        .rev()
        .filter(|item| item.target == "plan")
        .find_map(|item| {
            let entries = state
                .item(item)?
                .get("entries")?
                .as_array()
                .filter(|entries| !entries.is_empty())?;
            Some(
                entries
                    .iter()
                    .map(|entry| {
                        let status = Some(text(entry, "status"))
                            .filter(|status| !status.is_empty())
                            .unwrap_or("pending");
                        (text(entry, "content").to_owned(), status.to_owned())
                    })
                    .collect(),
            )
        })
}

/// What goes on beside the conversation, kept in view above the composer: the agent's todo list
/// with its progress, and what still runs in the background. `None` without either.
pub fn activity(key: &str, state: &SessionState, cx: &App) -> Option<Div> {
    let theme = theme(cx);
    let list = todos(state);
    let background = background_work(state);
    if list.is_none() && background.is_empty() {
        return None;
    }
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .w_full()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(theme.card_radius))
        .bg(theme.colors.card)
        .border_1()
        .border_color(theme.colors.line)
        .text_size(px(theme.text.secondary));
    if let Some(list) = list {
        let done = list
            .iter()
            .filter(|(_, status)| status == "completed")
            .count();
        let now = list
            .iter()
            .find(|(_, status)| status == "in_progress")
            .map(|(content, _)| format!(" · {content}"))
            .unwrap_or_default();
        let title = SharedString::from(format!("Tasks: {done} of {} done{now}", list.len()));
        column = column.child(fold(&format!("{key}/tasks"), title, false, cx, || {
            let mut entries = div().flex().flex_col().gap(px(2.0)).w_full();
            for (content, status) in &list {
                entries = entries.child(plan_entry(content, status, cx));
            }
            entries.into_any_element()
        }));
    }
    if !background.is_empty() {
        let mut row = div().flex().flex_wrap().items_center().gap(px(6.0)).child(
            div()
                .text_color(theme.colors.secondary)
                .child("Running in the background"),
        );
        for work in background {
            row = row.child(
                div()
                    .id(SharedString::from(format!("{key}/background/{}", work.id)))
                    .max_w(px(320.0))
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(theme.control_radius))
                    .bg(theme.colors.sunken)
                    .border_1()
                    .border_color(theme.colors.line)
                    .font_family(theme.text.mono_family.clone())
                    .text_size(px(theme.text.code))
                    .truncate()
                    .child(SharedString::from(work.label))
                    .tooltip(tip(work.kind)),
            );
        }
        column = column.child(row);
    }
    Some(column)
}

/// Another process that has the session open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenElsewhere {
    pub pid: Option<u64>,
    pub status: Option<String>,
}

/// Says that another process has the session open, so messages from here are refused, and
/// offers to continue on a copy. Without `fork` it only says so. `key` is where the kit
/// remembers that a fork is on its way, or why it failed.
pub fn open_elsewhere(key: &str, holder: &OpenElsewhere, fork: Option<Fork>, cx: &App) -> Div {
    let theme = theme(cx);
    let who = [
        holder
            .pid
            .filter(|pid| *pid > 0)
            .map(|pid| format!("pid {pid}")),
        holder.status.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    let who = if who.is_empty() {
        String::new()
    } else {
        format!(" ({who})")
    };
    let (forking_key, problem_key) = (format!("{key}/forking"), format!("{key}/problem"));
    let forking = memory::flag(cx, &forking_key, false);
    let failed = memory::text(cx, &problem_key);
    let (across, down) = theme.card_padding;
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .w_full()
        .px(px(across))
        .py(px(down))
        .rounded(px(theme.card_radius))
        .bg(theme.colors.attention.opacity(0.1))
        .border_1()
        .border_color(theme.colors.attention.opacity(0.7))
        .text_size(px(theme.text.secondary))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(format!("This session is open in another window of the agent{who}."))),
                )
                .child(div().text_color(theme.colors.secondary).child(SharedString::from(format!(
                    "Messages from here are refused so the two do not overwrite each other. Close it there and send again{}",
                    if fork.is_some() { ", or continue here on a copy." } else { "." }
                ))))
                .when_some(failed, |column, why| column.child(problem(&format!("Could not fork: {why}"), cx))),
        )
        .when_some(fork, |row, fork| {
            let title = if forking { "Forking…" } else { "Fork and continue here" };
            let one = button(format!("{key}/fork"), title, true, cx);
            if forking {
                return row.child(disabled(one));
            }
            row.child(one.on_click(move |_, window, cx| {
                memory::set_flag(cx, &forking_key, true);
                memory::set_text(cx, &problem_key, None);
                let work = fork(window, cx);
                let (forking_key, problem_key) = (forking_key.clone(), problem_key.clone());
                cx.spawn(async move |cx| {
                    let result = work.await;
                    cx.update(|cx| {
                        memory::set_flag(cx, &forking_key, false);
                        memory::set_text(cx, &problem_key, result.err());
                    })
                    .ok();
                })
                .detach();
            }))
        })
}
