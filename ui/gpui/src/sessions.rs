//! The sessions to choose from: grouped by folder, each with its state, and rename and delete on
//! the row.

use crate::input::{FieldEvent, TextField};
use crate::items::{shared, text};
use crate::parts::{Handler, Icon, badge, button, icon_button, label, link, problem, tip};
use crate::status::Outcome;
use crate::theme::{Tone, theme};
use gpui::{
    App, Context, Div, Entity, EventEmitter, Focusable as _, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px, relative,
};
use serde_json::Value;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

/// One session as the daemon lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionSummary {
    pub session_id: String,
    pub agent_id: String,
    pub title: Option<String>,
    pub cwd: String,
    /// False for a session started outside the app: opening it loads its history from the agent.
    pub logged: bool,
    pub open_elsewhere: bool,
    /// The turn: `idle`, `running`, `requires_action`.
    pub state: Option<String>,
    /// When it last changed, as the daemon writes it: `2026-11-24T11:55:00Z`.
    pub updated_at: Option<String>,
}

impl SessionSummary {
    /// From one entry of the daemon's session list. `None` without a session id.
    #[must_use]
    pub fn of(entry: &Value) -> Option<Self> {
        let optional = |field: &str| {
            Some(text(entry, field))
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let flag = |field: &str, default: bool| {
            entry.get(field).and_then(Value::as_bool).unwrap_or(default)
        };
        Some(Self {
            session_id: optional("sessionId")?,
            agent_id: optional("agentId").unwrap_or_default(),
            title: optional("title"),
            cwd: optional("cwd").unwrap_or_default(),
            logged: flag("logged", true),
            open_elsewhere: flag("openElsewhere", false),
            state: optional("state"),
            updated_at: optional("updatedAt"),
        })
    }

    /// How long ago the session changed, in a few characters: `now`, `5m`, `3h`, `2d`, then the
    /// date. `now` is in seconds since 1970.
    #[must_use]
    pub fn ago(&self, now: i64) -> String {
        let Some(updated_at) = &self.updated_at else {
            return String::new();
        };
        let Some(then) = unix_time(updated_at) else {
            return String::new();
        };
        match (now - then) / 60 {
            minutes if minutes < 1 => "now".to_owned(),
            minutes if minutes < 60 => format!("{minutes}m"),
            minutes if minutes < 60 * 24 => format!("{}h", minutes / 60),
            minutes if minutes < 60 * 24 * 30 => format!("{}d", minutes / (60 * 24)),
            _ => updated_at.chars().take(10).collect(),
        }
    }

    /// What a row says about the session's turn: only what is worth a look.
    #[must_use]
    pub fn attention(&self) -> Option<(&'static str, Tone)> {
        match self.state.as_deref() {
            Some("requires_action") => Some(("needs you", Tone::Attention)),
            Some("running") => Some(("running", Tone::Running)),
            _ => None,
        }
    }
}

/// Seconds since 1970 of a time written as `2026-11-24T11:55:00Z`, with or without fractions.
#[must_use]
pub fn unix_time(written: &str) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| written.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    // Days from the civil date, by Howard Hinnant's method.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// The time now, in seconds since 1970.
#[must_use]
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// The sessions by working folder, in the order the folders first appear.
#[must_use]
pub fn by_folder(sessions: &[SessionSummary]) -> Vec<(String, Vec<SessionSummary>)> {
    let mut folders: Vec<(String, Vec<SessionSummary>)> = Vec::new();
    for session in sessions {
        match folders.iter_mut().find(|(cwd, _)| *cwd == session.cwd) {
            Some((_, list)) => list.push(session.clone()),
            None => folders.push((session.cwd.clone(), vec![session.clone()])),
        }
    }
    folders
}

/// One session: its title, what it is doing, where else it lives, and how long ago it changed.
pub fn session_row(session: &SessionSummary, now: i64, cx: &App) -> Div {
    let theme = theme(cx);
    let id = &session.session_id;
    let mut details = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .w_full()
        .text_size(px(theme.text.meta))
        .text_color(theme.colors.secondary);
    if let Some((words, tone)) = session.attention() {
        details = details.child(badge(words, Some(tone), cx));
    }
    if session.open_elsewhere {
        details = details.child(
            div()
                .id(SharedString::from(format!("{id}/elsewhere")))
                .child(badge("open elsewhere", Some(Tone::Attention), cx))
                .tooltip(tip(
                    "Another window of the agent has this session open. Fork it to continue here.",
                )),
        );
    }
    details = if session.logged {
        details.child(div().min_w_0().truncate().child(shared(&session.agent_id)))
    } else {
        details.child(
            div()
                .id(SharedString::from(format!("{id}/from")))
                .min_w_0()
                .truncate()
                .child(SharedString::from(format!("from {}", session.agent_id)))
                .tooltip(tip(
                    "Started outside this app. Opening it loads its history from the agent.",
                )),
        )
    };
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .w_full()
                .truncate()
                .child(shared(session.title.as_deref().unwrap_or("Untitled"))),
        )
        .child(
            details.child(div().flex_1()).child(
                div()
                    .flex_none()
                    .child(SharedString::from(session.ago(now))),
            ),
        )
}

/// What a session list tells whoever subscribed to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionListEvent {
    /// The user chose a session.
    Opened(String),
}

/// Renames a session: its id and the new title.
pub type Rename = Rc<dyn Fn(&str, &str, &mut Window, &mut App) -> Outcome>;
/// Deletes a session. The agent removes it from its own history too and nobody can undo that,
/// so the list asks first.
pub type Delete = Rc<dyn Fn(&str, &mut Window, &mut App) -> Outcome>;

enum Editing {
    /// The session, the field, and the watch on the field, kept for as long as it is shown.
    Renaming(String, Entity<TextField>, #[allow(dead_code)] Subscription),
    Deleting(String),
}

pub struct SessionList {
    sessions: Vec<SessionSummary>,
    selected: Option<String>,
    /// The client's agents: one that does not say it can rename or delete gets no such button.
    /// An agent says so once it has been started. With `None`, every session gets them.
    agents: Option<Vec<Value>>,
    rename: Option<Rename>,
    delete: Option<Delete>,
    more: Option<Handler>,
    grouped: bool,
    /// The time "5m" counts from; `None` is the clock's.
    now: Option<i64>,
    editing: Option<Editing>,
    busy: Option<String>,
    /// The session something failed on, and what to say.
    failed: Option<(String, String)>,
}

impl EventEmitter<SessionListEvent> for SessionList {}

impl SessionList {
    #[must_use]
    pub fn new(sessions: Vec<SessionSummary>) -> Self {
        Self {
            sessions,
            selected: None,
            agents: None,
            rename: None,
            delete: None,
            more: None,
            grouped: true,
            now: None,
            editing: None,
            busy: None,
            failed: None,
        }
    }

    pub fn set_sessions(&mut self, sessions: Vec<SessionSummary>, cx: &mut Context<Self>) {
        self.sessions = sessions;
        cx.notify();
    }

    /// The session that is open.
    pub fn set_selected(&mut self, selected: Option<String>, cx: &mut Context<Self>) {
        self.selected = selected;
        cx.notify();
    }

    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn set_agents(&mut self, agents: Option<Vec<Value>>, cx: &mut Context<Self>) {
        self.agents = agents;
        cx.notify();
    }

    pub fn set_rename(&mut self, rename: Option<Rename>, cx: &mut Context<Self>) {
        self.rename = rename;
        cx.notify();
    }

    pub fn set_delete(&mut self, delete: Option<Delete>, cx: &mut Context<Self>) {
        self.delete = delete;
        cx.notify();
    }

    /// There are more sessions to ask the daemon for: draws "Show more".
    pub fn set_more(&mut self, more: Option<Handler>, cx: &mut Context<Self>) {
        self.more = more;
        cx.notify();
    }

    /// Under each folder's name (the default), or one flat list.
    pub fn set_grouped(&mut self, grouped: bool, cx: &mut Context<Self>) {
        self.grouped = grouped;
        cx.notify();
    }

    pub fn set_now(&mut self, now: Option<i64>, cx: &mut Context<Self>) {
        self.now = now;
        cx.notify();
    }

    /// Whether the session's agent said it can do this (`title` or `delete`).
    fn can(&self, session: &SessionSummary, what: &str) -> bool {
        let Some(agents) = &self.agents else {
            return true;
        };
        agents
            .iter()
            .find(|agent| text(agent, "agentId") == session.agent_id)
            .and_then(|agent| agent.pointer(&format!("/capabilities/session/{what}")))
            .is_some_and(|said| !said.is_null())
    }

    /// Shows the field to rename a session in its row.
    pub fn start_renaming(
        &mut self,
        session_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self
            .sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .and_then(|session| session.title.clone())
            .unwrap_or_default();
        let field = cx.new(|cx| TextField::new("Title", cx).with_text(title));
        let watch = cx.subscribe_in(&field, window, |list, _, event, window, cx| match event {
            FieldEvent::Submitted => list.save_title(window, cx),
            FieldEvent::Cancelled => list.stop_editing(cx),
            FieldEvent::Changed => {}
        });
        window.focus(&field.read(cx).focus_handle(cx));
        self.failed = None;
        self.editing = Some(Editing::Renaming(session_id.to_owned(), field, watch));
        cx.notify();
    }

    /// Asks before deleting a session, in its row.
    pub fn start_deleting(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.failed = None;
        self.editing = Some(Editing::Deleting(session_id.to_owned()));
        cx.notify();
    }

    fn stop_editing(&mut self, cx: &mut Context<Self>) {
        self.editing = None;
        cx.notify();
    }

    fn save_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Editing::Renaming(id, field, _)) = self.editing.take() else {
            return;
        };
        let title = field.read(cx).text().trim().to_owned();
        let current = self
            .sessions
            .iter()
            .find(|session| session.session_id == id)
            .and_then(|session| session.title.clone());
        cx.notify();
        if title.is_empty() || Some(&title) == current.as_ref() {
            return;
        }
        if let Some(rename) = self.rename.clone() {
            let work = rename(&id, &title, window, cx);
            self.run(id, "Not renamed", work, cx);
        }
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Editing::Deleting(id)) = self.editing.take() else {
            return;
        };
        cx.notify();
        if let Some(delete) = self.delete.clone() {
            let work = delete(&id, window, cx);
            self.run(id, "Not deleted", work, cx);
        }
    }

    fn run(&mut self, id: String, what: &'static str, work: Outcome, cx: &mut Context<Self>) {
        self.busy = Some(id.clone());
        cx.spawn(async move |list, cx| {
            let result = work.await;
            list.update(cx, |list, cx| {
                list.busy = None;
                list.failed = result.err().map(|why| (id, format!("{what}: {why}")));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn row(&self, session: &SessionSummary, now: i64, cx: &mut Context<Self>) -> Div {
        let theme = theme(cx);
        let id = session.session_id.clone();
        let selected = self.selected.as_deref() == Some(&id);
        let busy = self.busy.as_deref() == Some(&id);
        let mut column = div().flex().flex_col().gap(px(4.0)).w_full();
        match &self.editing {
            Some(Editing::Renaming(editing, field, _)) if *editing == id => {
                column = column.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .w_full()
                        .px(px(8.0))
                        .py(px(6.0))
                        .child(div().flex_1().min_w_0().child(field.clone()))
                        .child(button(format!("{id}/save"), "Save", true, cx).on_click(
                            cx.listener(|list, _, window, cx| list.save_title(window, cx)),
                        ))
                        .child(
                            button(format!("{id}/cancel"), "Cancel", false, cx)
                                .on_click(cx.listener(|list, _, _, cx| list.stop_editing(cx))),
                        ),
                );
            }
            Some(Editing::Deleting(editing)) if *editing == id => {
                let title = session.title.as_deref().unwrap_or("Untitled");
                column = column.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .w_full()
                        .px(px(8.0))
                        .py(px(8.0))
                        .rounded(px(theme.control_radius))
                        .bg(theme.colors.bad.opacity(0.1))
                        .text_size(px(theme.text.secondary))
                        .child(SharedString::from(format!(
                            "Delete “{title}”? It is removed from {}’s own history too. This cannot be undone.",
                            session.agent_id
                        )))
                        .child(
                            div()
                                .flex()
                                .gap(px(6.0))
                                .child(button(format!("{id}/delete"), "Delete", true, cx).on_click(cx.listener(|list, _, window, cx| list.confirm_delete(window, cx))))
                                .child(button(format!("{id}/keep"), "Cancel", false, cx).on_click(cx.listener(|list, _, _, cx| list.stop_editing(cx)))),
                        ),
                );
            }
            _ => {
                let group = SharedString::from(format!("row/{id}"));
                let mut actions = div()
                    .flex()
                    .flex_none()
                    .invisible()
                    .group_hover(group.clone(), |actions| actions.visible());
                if self.rename.is_some() && self.can(session, "title") {
                    let id = id.clone();
                    actions = actions.child(
                        icon_button(format!("{id}/rename"), Icon::Pencil, "Rename", cx).on_click(
                            cx.listener(move |list, _, window, cx| {
                                cx.stop_propagation();
                                list.start_renaming(&id, window, cx);
                            }),
                        ),
                    );
                }
                if self.delete.is_some() && self.can(session, "delete") {
                    let id = id.clone();
                    actions = actions.child(
                        icon_button(format!("{id}/remove"), Icon::Trash, "Delete", cx).on_click(
                            cx.listener(move |list, _, _, cx| {
                                cx.stop_propagation();
                                list.start_deleting(&id, cx);
                            }),
                        ),
                    );
                }
                let open = id.clone();
                column = column.child(
                    div()
                        .id(SharedString::from(format!("{id}/row")))
                        .debug_selector(|| format!("{id}/row"))
                        .group(group)
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .w_full()
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(theme.control_radius))
                        .cursor_pointer()
                        .when(selected, |row| row.bg(theme.colors.running.opacity(0.16)))
                        .when(!selected, |row| row.hover(|row| row.bg(theme.colors.card)))
                        .when(busy, |row| row.opacity(0.5))
                        .child(session_row(session, now, cx))
                        .child(actions)
                        .on_click(cx.listener(move |list, _, _, cx| {
                            list.selected = Some(open.clone());
                            cx.emit(SessionListEvent::Opened(open.clone()));
                            cx.notify();
                        })),
                );
            }
        }
        if let Some((failed, why)) = &self.failed
            && *failed == id
        {
            column = column.child(div().px(px(8.0)).child(problem(why, cx)));
        }
        column
    }
}

impl Render for SessionList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        let now = self.now.unwrap_or_else(now);
        let mut column = div()
            .id("sessions")
            .flex()
            .flex_col()
            .gap(px(2.0))
            .size_full()
            .overflow_y_scroll()
            .p(px(8.0))
            .bg(theme.colors.background)
            .text_color(theme.colors.text)
            .text_size(px(theme.text.body))
            .line_height(relative(theme.text.line_height))
            .when_some(theme.text.family.clone(), |column, family| {
                column.font_family(family)
            });
        if self.sessions.is_empty() {
            return column
                .items_center()
                .justify_center()
                .text_color(theme.colors.secondary)
                .child("No sessions");
        }
        let sessions = self.sessions.clone();
        if self.grouped {
            for (cwd, list) in by_folder(&sessions) {
                let name = cwd
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(&cwd)
                    .to_owned();
                let heading = if theme.folder_labels {
                    label(&name, cx)
                } else {
                    div()
                        .text_size(px(theme.text.secondary))
                        .text_color(theme.colors.secondary)
                        .child(shared(&name))
                };
                column = column.child(
                    div()
                        .id(SharedString::from(format!("folder/{cwd}")))
                        .px(px(8.0))
                        .pt(px(10.0))
                        .pb(px(4.0))
                        .child(heading)
                        .tooltip(tip(&cwd)),
                );
                for session in &list {
                    column = column.child(self.row(session, now, cx));
                }
            }
        } else {
            for session in &sessions {
                column = column.child(self.row(session, now, cx));
            }
        }
        if let Some(more) = self.more.clone() {
            column = column.child(div().px(px(8.0)).py(px(8.0)).child(
                link("more", "Show more", cx).on_click(move |_, window, cx| more(window, cx)),
            ));
        }
        column
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_time_is_read_and_told_shortly() {
        assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_time("2026-11-24T12:00:00.250Z"), Some(1_795_521_600));
        let now = unix_time("2026-11-24T12:00:00Z").unwrap_or(0);
        let ago = |at: &str| {
            SessionSummary {
                updated_at: Some(at.to_owned()),
                ..SessionSummary::default()
            }
            .ago(now)
        };
        assert_eq!(ago("2026-11-24T11:59:40Z"), "now");
        assert_eq!(ago("2026-11-24T11:55:00Z"), "5m");
        assert_eq!(ago("2026-11-24T08:59:59Z"), "3h");
        assert_eq!(ago("2026-11-22T12:00:00Z"), "2d");
        assert_eq!(ago("2026-08-01T12:00:00Z"), "2026-08-01");
    }

    #[test]
    fn sessions_are_read_and_grouped_by_folder() {
        let sessions: Vec<_> = [
            json!({ "sessionId": "s1", "agentId": "claude", "cwd": "/work/app", "title": "Fix", "state": "requires_action" }),
            json!({ "sessionId": "s2", "agentId": "claude", "cwd": "/work/site", "logged": false }),
            json!({ "sessionId": "s3", "agentId": "pi", "cwd": "/work/app", "state": "running" }),
            json!({ "agentId": "nobody" }),
        ]
        .iter()
        .filter_map(SessionSummary::of)
        .collect();
        assert_eq!(sessions.len(), 3);
        assert_eq!(
            sessions[0].attention(),
            Some(("needs you", Tone::Attention))
        );
        assert!(!sessions[1].logged);
        let folders = by_folder(&sessions);
        assert_eq!(
            folders
                .iter()
                .map(|(cwd, list)| (cwd.as_str(), list.len()))
                .collect::<Vec<_>>(),
            [("/work/app", 2), ("/work/site", 1)]
        );
    }
}
