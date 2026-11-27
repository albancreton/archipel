//! One session, assembled: the status bar, the timeline, what goes on beside it, the settings
//! and the composer. An app that wants another layout assembles its own from the parts.

use crate::composer::{Accepts, Command, Composer, FindMentions, Send};
use crate::items::{self, text};
use crate::memory;
use crate::parts::{Handler, problem};
use crate::settings::{Change, Setting, settings_bar};
use crate::status::{Fork, OpenElsewhere, activity, open_elsewhere, status_bar};
use crate::theme::theme;
use archipel_core::state::{Conversation, ItemRef, REQUEST_TARGET, SessionState};
use gpui::{
    AnyElement, App, Context, Div, Entity, IntoElement, ListAlignment, ListState, Render, Window,
    div, list, prelude::*, px, relative,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Answers an approval: the request's id, and the id of the option taken.
pub type Approve = Rc<dyn Fn(&str, &str, &mut Window, &mut App)>;
/// Answers a question: the request's id, and the form's content or `None` to skip it.
pub type AnswerRequest = Rc<dyn Fn(&str, Option<Value>, &mut Window, &mut App)>;

/// What the user can do. Each is optional; without it, its control is not drawn.
#[derive(Clone, Default)]
pub struct ChatActions {
    /// Answer an approval with one of its options: request id, option id.
    pub approve: Option<Approve>,
    /// Answer a question: request id, and the form's content or `None` to skip it.
    pub answer: Option<AnswerRequest>,
    /// Send a message, as content blocks. If it fails, the composer gives everything back.
    pub prompt: Option<Send>,
    /// Stop the running turn.
    pub cancel: Option<Handler>,
    /// Change a setting of the agent, such as the model. With it the chat draws the settings bar.
    pub set_option: Option<Change>,
    /// Continue on a copy of the session, when another process has this one open.
    pub fork: Option<Fork>,
}

/// Views for kinds of item that this kit does not know, by target, such as `x.acme.checkpoint`.
pub type ItemViews = HashMap<String, Rc<dyn Fn(&Value, &mut Window, &mut App) -> AnyElement>>;

/// The terminals that a tool call of the session shows: they are not drawn a second time.
#[must_use]
pub fn inline_terminals(state: &SessionState) -> HashSet<String> {
    state
        .collections
        .get("toolCall")
        .into_iter()
        .flat_map(|tools| tools.values())
        .flat_map(|tool| {
            tool.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .map(|block| text(block, "terminalId"))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether a tool call of the session still runs: its status says so, and either the turn is
/// not idle or the call is in the turn's background work. A call left `in_progress` by a turn
/// that was cut short is over.
#[must_use]
pub fn tool_runs(state: &SessionState, id: &str, item: &Value) -> bool {
    if !items::tool_running(item) {
        return false;
    }
    let Some(turn) = state.values.get("state") else {
        return true;
    };
    text(turn, "state") != "idle"
        || turn
            .get("background")
            .and_then(Value::as_array)
            .is_some_and(|work| work.iter().any(|work| text(work, "toolCallId") == id))
}

/// A row of a timeline: one entry, or finished tool calls that followed one another (with what
/// came between them) drawn as one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineRow {
    Item(ItemRef),
    Group(Vec<ItemRef>),
}

/// The rows of a session's timeline, from its current state, in order.
///
/// - A call that is over joins the group being built. A group needs two calls; one alone is a
///   plain row.
/// - An approval that has its answer goes into the group when it directly follows one of its
///   calls (or another approval taken in). A thought goes in only when another call follows it,
///   and so does an answered approval that comes after such a thought.
/// - Anything else ends the group, and so does a call that still runs, which stays on its own.
/// - What draws nothing (a cursor, a terminal its tool call shows, an entry with no item) is
///   passed over and ends nothing.
#[must_use]
pub fn timeline_rows(state: &SessionState, inline: &HashSet<String>) -> Vec<TimelineRow> {
    fn close(rows: &mut Vec<TimelineRow>, group: &mut Vec<ItemRef>, after: &mut Vec<ItemRef>) {
        let calls = group.iter().filter(|entry| entry.target == "toolCall");
        if calls.count() > 1 {
            rows.push(TimelineRow::Group(std::mem::take(group)));
        } else {
            rows.extend(group.drain(..).map(TimelineRow::Item));
        }
        rows.extend(after.drain(..).map(TimelineRow::Item));
    }
    let mut rows = Vec::new();
    let mut group: Vec<ItemRef> = Vec::new();
    // What followed the group's last call, until another call takes it in.
    let mut after: Vec<ItemRef> = Vec::new();
    for entry in &state.timeline {
        let answered = if entry.target == REQUEST_TARGET {
            let Some(request) = state.requests.get(&entry.id) else {
                continue;
            };
            request.method == "session/request_permission" && request.outcome.is_some()
        } else {
            let Some(item) = state.item(entry) else {
                continue;
            };
            match entry.target.as_str() {
                "cursor" => continue,
                "terminal" if inline.contains(&entry.id) => continue,
                "toolCall" if !tool_runs(state, &entry.id, item) => {
                    group.append(&mut after);
                    group.push(entry.clone());
                    continue;
                }
                _ => false,
            }
        };
        if answered && !group.is_empty() && after.is_empty() {
            group.push(entry.clone());
        } else if !group.is_empty() && (answered || entry.target == "agentThought") {
            after.push(entry.clone());
        } else {
            close(&mut rows, &mut group, &mut after);
            rows.push(TimelineRow::Item(entry.clone()));
        }
    }
    close(&mut rows, &mut group, &mut after);
    rows
}

/// The key a group's open state is remembered under: its first row's, so a group the user
/// opened stays open while calls are added to it.
fn group_key(session_id: &str, entries: &[ItemRef]) -> String {
    let first = entries.first().map_or("", |entry| entry.id.as_str());
    format!("{session_id}/toolGroup/{first}")
}

/// One row of [`timeline_rows`], drawn: an entry by its kind, or a group of tool calls.
#[allow(clippy::too_many_arguments)]
pub fn timeline_row_of(
    conversation: &Conversation,
    session_id: &str,
    state: &SessionState,
    row: &TimelineRow,
    inline: &HashSet<String>,
    actions: &ChatActions,
    custom: &ItemViews,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let entries = match row {
        TimelineRow::Item(entry) => {
            return timeline_row(
                conversation,
                session_id,
                state,
                entry,
                inline,
                actions,
                custom,
                window,
                cx,
            );
        }
        TimelineRow::Group(entries) => entries,
    };
    let calls: Vec<&Value> = entries
        .iter()
        .filter(|entry| entry.target == "toolCall")
        .filter_map(|entry| state.item(entry))
        .collect();
    let key = group_key(session_id, entries);
    // The rows are built only when the group is open. Building needs the app mutably (a
    // question's fields), which a fold's body does not have, so it is done before the fold.
    let rows: Vec<AnyElement> = if memory::flag(cx, &key, false) {
        entries
            .iter()
            .filter_map(|entry| {
                timeline_row(
                    conversation,
                    session_id,
                    state,
                    entry,
                    inline,
                    actions,
                    custom,
                    window,
                    cx,
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(items::tool_group(&key, &calls, cx, || rows).into_any_element())
}

/// One row of a timeline: the item `entry` points at, drawn by its kind. `None` for what is not
/// drawn (a cursor, a terminal a tool call shows, an entry with no item).
#[allow(clippy::too_many_arguments)]
pub fn timeline_row(
    conversation: &Conversation,
    session_id: &str,
    state: &SessionState,
    entry: &ItemRef,
    inline: &HashSet<String>,
    actions: &ChatActions,
    custom: &ItemViews,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let key = format!("{session_id}/{}/{}", entry.target, entry.id);
    if entry.target == REQUEST_TARGET {
        let request = state.requests.get(&entry.id)?;
        let id = entry.id.clone();
        return Some(if request.method == "session/request_permission" {
            let choose = actions.approve.clone().map(|approve| {
                Rc::new(move |option: &str, window: &mut Window, cx: &mut App| {
                    approve(&id, option, window, cx)
                }) as items::Choose
            });
            items::approval(&key, request, choose, cx).into_any_element()
        } else {
            let answer = actions.answer.clone().map(|answer| {
                Rc::new(
                    move |content: Option<Value>, window: &mut Window, cx: &mut App| {
                        answer(&id, content, window, cx)
                    },
                ) as items::Answer
            });
            items::question(&key, request, answer, window, cx).into_any_element()
        });
    }
    let item = state.item(entry)?;
    if let Some(view) = custom.get(&entry.target) {
        return Some(view(item, window, cx));
    }
    Some(
        match entry.target.as_str() {
            "userMessage" => items::user_message(item, cx),
            "agentMessage" => items::assistant_message(&key, item, window, cx),
            "agentThought" => items::thought(&key, item, window, cx),
            "toolCall" => items::tool_call(
                &key,
                item,
                tool_runs(state, &entry.id, item),
                |id| state.collections.get("terminal")?.get(id),
                cx,
            ),
            "terminal" if inline.contains(&entry.id) => return None,
            "terminal" => items::terminal(item, cx),
            "plan" => items::plan(item, cx),
            "sessionMessage" => items::session_message(item, cx),
            "compaction" => items::compaction(&key, item, cx),
            "subagent" => {
                let inner = conversation.sessions.get(&entry.id).map(|child| {
                    timeline(conversation, &entry.id, child, actions, custom, window, cx)
                        .into_any_element()
                });
                items::subagent(item, inner, cx)
            }
            // A cursor marks a place in the history; it is not content.
            "cursor" => return None,
            other => items::fallback(&key, other, item, cx),
        }
        .into_any_element(),
    )
}

/// The space a row takes above and below itself, besides the rows' own spacing. A turn starts
/// at the user's message: with this it stands four times the spacing from its neighbours,
/// except above the first row.
fn turn_space(row: &TimelineRow, index: usize, spacing: f32) -> (f32, f32) {
    match row {
        TimelineRow::Item(entry) if entry.target == "userMessage" => {
            (if index == 0 { 0.0 } else { 3.0 * spacing }, 3.0 * spacing)
        }
        _ => (0.0, 0.0),
    }
}

/// The items of one session, in order, all of them, finished tool calls in a row as one group.
/// A subagent's session is drawn inside its item. [`ChatView`] draws only the rows on screen; use this for a short timeline of your own.
pub fn timeline(
    conversation: &Conversation,
    session_id: &str,
    state: &SessionState,
    actions: &ChatActions,
    custom: &ItemViews,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let inline = inline_terminals(state);
    let spacing = theme(cx).row_spacing;
    let rows: Vec<AnyElement> = timeline_rows(state, &inline)
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let drawn = timeline_row_of(
                conversation,
                session_id,
                state,
                row,
                &inline,
                actions,
                custom,
                window,
                cx,
            )?;
            let (above, below) = turn_space(row, index, spacing);
            Some(
                div()
                    .w_full()
                    .pt(px(above))
                    .pb(px(below))
                    .child(drawn)
                    .into_any_element(),
            )
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(theme(cx).row_spacing))
        .w_full()
        .children(rows)
}

/// A chat on one session. Give it the conversation again after each change
/// ([`ChatView::set_conversation`]): it draws the items as they are now.
pub struct ChatView {
    conversation: Rc<Conversation>,
    session_id: Option<String>,
    actions: ChatActions,
    custom: Rc<ItemViews>,
    open_elsewhere: Option<OpenElsewhere>,
    error: Option<String>,
    inline: Rc<HashSet<String>>,
    rows: ListState,
    /// The session and the rows the list was told of: worked out once per change of state, not
    /// per row drawn.
    listed: (Option<String>, Rc<Vec<TimelineRow>>),
    composer: Option<Entity<Composer>>,
}

impl ChatView {
    pub fn new(
        conversation: Conversation,
        actions: ChatActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = actions
            .prompt
            .clone()
            .map(|send| cx.new(|cx| Composer::new(send, window, cx)));
        let mut view = Self {
            conversation: Rc::new(Conversation::default()),
            session_id: None,
            actions,
            custom: Rc::new(ItemViews::new()),
            open_elsewhere: None,
            error: None,
            inline: Rc::new(HashSet::new()),
            // A chat opens at its newest item, and stays there while more arrive.
            rows: ListState::new(0, ListAlignment::Bottom, px(600.0)),
            listed: (None, Rc::new(Vec::new())),
            composer,
        };
        view.set_conversation(conversation, cx);
        view
    }

    /// The id of the session that is drawn: the one that was set, or the conversation's first.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id
            .as_deref()
            .or_else(|| self.conversation.root().map(|(id, _)| id.as_str()))
    }

    fn state(&self) -> Option<&SessionState> {
        self.conversation.sessions.get(self.session_id()?)
    }

    /// The state after a change. Rows are added to the list; what changed in place is drawn again.
    pub fn set_conversation(&mut self, conversation: Conversation, cx: &mut Context<Self>) {
        self.conversation = Rc::new(conversation);
        self.sync(cx);
    }

    /// Which session of the conversation to draw; `None` is the first.
    pub fn set_session(&mut self, session_id: Option<String>, cx: &mut Context<Self>) {
        self.session_id = session_id;
        self.sync(cx);
    }

    /// The app's components for kinds it knows, by target.
    pub fn set_custom(&mut self, custom: ItemViews, cx: &mut Context<Self>) {
        self.custom = Rc::new(custom);
        cx.notify();
    }

    /// Set while another process has the session open: the chat says so and offers the fork.
    pub fn set_open_elsewhere(&mut self, holder: Option<OpenElsewhere>, cx: &mut Context<Self>) {
        self.open_elsewhere = holder;
        cx.notify();
    }

    /// Something the daemon reported outside any request, such as a log it cannot write.
    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    /// The composer, to give it what the agent accepts, a way to find mentions, or a draft.
    #[must_use]
    pub fn composer(&self) -> Option<&Entity<Composer>> {
        self.composer.as_ref()
    }

    pub fn set_mentions(&mut self, mentions: Option<FindMentions>, cx: &mut Context<Self>) {
        if let Some(composer) = &self.composer {
            composer.update(cx, |composer, cx| composer.set_mentions(mentions, cx));
        }
    }

    pub fn set_accepts(&mut self, accepts: Accepts, cx: &mut Context<Self>) {
        if let Some(composer) = &self.composer {
            composer.update(cx, |composer, cx| composer.set_accepts(accepts, cx));
        }
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let session = self.session_id().map(str::to_owned);
        let (rows, inline, running, commands) = match self.state() {
            Some(state) => {
                let inline = inline_terminals(state);
                (
                    timeline_rows(state, &inline),
                    inline,
                    state
                        .values
                        .get("state")
                        .map(|turn| text(turn, "state"))
                        .is_some_and(|turn| !turn.is_empty() && turn != "idle"),
                    Command::all(state.values.get("availableCommands")),
                )
            }
            None => (Vec::new(), HashSet::new(), false, Vec::new()),
        };
        self.inline = Rc::new(inline);
        if self.listed.0 != session {
            self.rows.reset(rows.len());
        } else {
            // Rows are added at the end, and the last ones change when calls end and join a
            // group: the list is told of what differs from the first row that does.
            let old = &self.listed.1;
            let same = old.iter().zip(&rows).take_while(|(a, b)| a == b).count();
            if same < old.len() || same < rows.len() {
                self.rows.splice(same..old.len(), rows.len() - same);
            }
        }
        self.listed = (session, Rc::new(rows));
        if let Some(composer) = &self.composer {
            let cancel = self.actions.cancel.clone();
            composer.update(cx, |composer, cx| {
                composer.set_running(running, cx);
                composer.set_commands(commands, cx);
                composer.set_cancel(cancel, cx);
            });
        }
        cx.notify();
    }
}

impl Render for ChatView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        let base = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.colors.background)
            .text_color(theme.colors.text)
            .text_size(px(theme.text.body))
            .line_height(relative(theme.text.line_height))
            .when_some(theme.text.family.clone(), |base, family| {
                base.font_family(family)
            });
        let Some(session_id) = self.session_id().map(str::to_owned) else {
            return base
                .items_center()
                .justify_center()
                .text_color(theme.colors.secondary)
                .child("No session");
        };
        // A session that was just made holds nothing yet: it is drawn empty, with its composer,
        // since the first message is what fills it.
        let nothing = SessionState::default();
        let state = self
            .conversation
            .sessions
            .get(&session_id)
            .unwrap_or(&nothing);
        let (conversation, actions, custom, inline, listed) = (
            self.conversation.clone(),
            self.actions.clone(),
            self.custom.clone(),
            self.inline.clone(),
            self.listed.1.clone(),
        );
        let (id, spacing) = (session_id.clone(), theme.row_spacing);
        // Only the rows on screen are built, so a long session opens as quickly as a short one.
        let rows = list(self.rows.clone(), move |index, window, cx| {
            let row = conversation.sessions.get(&id).and_then(|state| {
                timeline_row_of(
                    &conversation,
                    &id,
                    state,
                    listed.get(index)?,
                    &inline,
                    &actions,
                    &custom,
                    window,
                    cx,
                )
            });
            match row {
                Some(row) => {
                    let (above, below) = listed
                        .get(index)
                        .map_or((0.0, 0.0), |listed| turn_space(listed, index, spacing));
                    div()
                        .w_full()
                        .px(px(12.0))
                        .pt(px(above))
                        .pb(px(spacing + below))
                        .child(row)
                        .into_any_element()
                }
                None => div().into_any_element(),
            }
        })
        .size_full();
        let below = |part: Div| div().w_full().px(px(12.0)).pb(px(12.0)).child(part);
        base.child(
            div()
                .w_full()
                .p(px(12.0))
                .border_b_1()
                .border_color(theme.colors.line)
                .child(status_bar(state, cx)),
        )
        .child(div().flex_1().min_h_0().w_full().pt(px(12.0)).child(rows))
        .children(activity(&session_id, state, cx).map(below))
        .children(self.open_elsewhere.as_ref().map(|holder| {
            below(open_elsewhere(
                &format!("{session_id}/elsewhere"),
                holder,
                self.actions.fork.clone(),
                cx,
            ))
        }))
        .children(self.error.as_deref().map(|error| below(problem(error, cx))))
        .children(self.actions.set_option.clone().and_then(|change| {
            settings_bar(
                &format!("{session_id}/settings"),
                &Setting::all(state.values.get("configOptions")),
                Some(change),
                cx,
            )
            .map(below)
        }))
        .children(self.composer.clone().map(|composer| {
            div()
                .w_full()
                .p(px(12.0))
                .border_t_1()
                .border_color(theme.colors.line)
                .child(composer)
        }))
    }
}

/// Forgets what the kit remembered for a session (open folds, form values): call it when the
/// session is closed for good.
pub fn forget(session_id: &str, cx: &mut App) {
    memory::forget(cx, &format!("{session_id}/"));
}
