//! One function per kind of item. Each takes the item (JSON as the client holds it) and, where
//! the user can act, a callback. Nothing here knows about connections: give them state and they
//! draw it.
//!
//! A `key` names the item among the others on screen, such as `"<session id>/<item id>"`: the
//! kit remembers folds and form values under it.

use crate::diff::diff;
use crate::input::TextField;
use crate::markdown::markdown;
use crate::memory;
use crate::parts::{
    Icon, MenuChoice, MenuGroup, badge, button, card, code_block, fold, guided, guided_fold, icon,
    label, menu, plan_mark, terminal_block,
};
use crate::theme::theme;
use archipel_core::state::Request;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{
    AnyElement, App, Div, FontWeight, Image, ImageFormat, SharedString, Window, div, img,
    prelude::*, px, relative,
};
use serde_json::{Map, Value, json};
use std::rc::Rc;
use std::sync::Arc;

/// The string at `field`, or "".
pub(crate) fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or("")
}

pub(crate) fn shared(text: &str) -> SharedString {
    SharedString::from(text.to_owned())
}

/// The text of a content field: a list of blocks, one block, or a string.
#[must_use]
pub fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|block| content_text(Some(block)))
            .collect(),
        Some(block @ Value::Object(_)) => match block.get("type").and_then(Value::as_str) {
            Some("text") => text(block, "text").to_owned(),
            Some("content") => content_text(block.get("content")),
            other => format!("[{}]", other.unwrap_or("content")),
        },
        _ => String::new(),
    }
}

/// A picture from bytes the user sent, when they are of a kind GPUI draws.
fn picture(block: &Value) -> Option<Arc<Image>> {
    let format = match text(block, "mimeType") {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        "image/bmp" => ImageFormat::Bmp,
        "image/tiff" => ImageFormat::Tiff,
        _ => return None,
    };
    let bytes = BASE64.decode(text(block, "data")).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

/// What the user sent: the text as written, on the user's side, then a picture for each image
/// (from its own bytes) and the name of each file or mention.
pub fn user_message(item: &Value, cx: &App) -> Div {
    let theme = theme(cx);
    let blocks: Vec<&Value> = match item.get("content") {
        Some(Value::Array(blocks)) => blocks.iter().collect(),
        Some(one) => vec![one],
        None => Vec::new(),
    };
    let is_text = |block: &Value| block.is_string() || text(block, "type") == "text";
    let written: String = blocks
        .iter()
        .filter(|block| is_text(block))
        .map(|block| content_text(Some(block)))
        .collect();
    let mut bubble = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .max_w(relative(0.85))
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(14.0))
        .bg(theme.colors.message)
        .text_color(theme.colors.on_message);
    if !written.is_empty() {
        bubble = bubble.child(shared(&written));
    }
    for block in blocks.iter().filter(|block| !is_text(block)) {
        if text(block, "type") == "image"
            && let Some(picture) = picture(block)
        {
            bubble = bubble.child(
                img(picture)
                    .max_w(px(220.0))
                    .max_h(px(160.0))
                    .rounded(px(8.0)),
            );
            continue;
        }
        let uri = text(block.get("resource").unwrap_or(block), "uri");
        let name = [
            text(block, "name"),
            uri.rsplit('/').next().unwrap_or(""),
            text(block, "type"),
            "attachment",
        ]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or("attachment");
        bubble = bubble.child(
            div()
                .flex_none()
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(8.0))
                .bg(theme.colors.on_message.opacity(0.2))
                .text_size(px(theme.text.meta))
                .child(shared(name)),
        );
    }
    div().flex().w_full().justify_end().child(bubble)
}

/// What the agent said, as Markdown: the strongest text of a turn, in the primary colour and
/// the largest size, so that it stands out from what the agent did and thought.
pub fn assistant_message(key: &str, item: &Value, window: &Window, cx: &App) -> Div {
    let theme = theme(cx);
    markdown(
        key,
        &content_text(item.get("content")),
        theme.colors.text,
        window,
        cx,
    )
    .text_size(px(theme.text.answer))
}

/// The agent's reasoning: a fold titled "Thinking", closed. The same quiet line as a tool call.
pub fn thought(key: &str, item: &Value, window: &Window, cx: &App) -> Div {
    let theme = theme(cx);
    fold(key, "Thinking", false, cx, || {
        markdown(
            key,
            content_text(item.get("content")).trim(),
            theme.colors.secondary,
            window,
            cx,
        )
        .text_size(px(theme.text.secondary))
        .into_any_element()
    })
}

/// How a terminal ended, as its badge says it.
#[must_use]
pub fn terminal_status(item: &Value) -> String {
    match item.get("exitStatus") {
        Some(exit @ Value::Object(_)) => exit
            .get("exitCode")
            .and_then(Value::as_i64)
            .map_or_else(|| "ended".to_owned(), |code| format!("exit {code}")),
        _ => "running".to_owned(),
    }
}

/// A command and its output: the command line, how it ended, and what it printed.
pub fn terminal(item: &Value, cx: &App) -> Div {
    let theme = theme(cx);
    let output = item
        .pointer("/output/data")
        .and_then(Value::as_str)
        .and_then(|data| BASE64.decode(data).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .w_full()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(theme.text.mono_family.clone())
                        .text_size(px(theme.text.mono))
                        .child(SharedString::from(format!("$ {}", text(item, "command")))),
                )
                .child(badge(&terminal_status(item), None, cx)),
        )
        .when(!output.trim_matches('\n').is_empty(), |column| {
            column.child(terminal_block(&output, cx))
        })
}

/// Whether a tool call is still going, by its own status.
#[must_use]
pub fn tool_running(item: &Value) -> bool {
    matches!(text(item, "status"), "" | "pending" | "in_progress")
}

/// Whether a tool call failed or was cancelled.
#[must_use]
pub fn tool_failed(item: &Value) -> bool {
    matches!(text(item, "status"), "failed" | "cancelled")
}

/// Something the agent did with a tool: one quiet line with its kind, title and status (not a
/// card), and under it what it produced. The output shows while the call runs and folds away
/// when it is over; the line opens and closes it, and what the user chose stays, remembered
/// under `key`. A call with nothing to show is the line alone, and it is not a control.
///
/// `running` is whether the call still runs: [`tool_running`] for a call on its own, or what the
/// timeline knows (a call left `in_progress` by a turn that was cut short is over).
/// `find_terminal` finds a terminal item by id, for calls whose content points at one.
pub fn tool_call<'a>(
    key: &str,
    item: &Value,
    running: bool,
    find_terminal: impl Fn(&str) -> Option<&'a Value>,
    cx: &App,
) -> Div {
    let theme = theme(cx);
    let name = |field: &str| Some(text(item, field)).filter(|name| !name.is_empty());
    let status = name("status").unwrap_or("pending");
    let blocks = item
        .get("content")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let shows = |block: &Value| match text(block, "type") {
        "terminal" => find_terminal(text(block, "terminalId")).is_some(),
        "diff" => true,
        _ => !content_text(Some(block)).is_empty(),
    };
    let has_output = blocks.iter().any(shows);
    // Closed, the title is one line: the first of a command written on several. Open, it is
    // all there, so a call with such a title opens even when it produced nothing.
    let title = name("title").or(name("name")).unwrap_or("").trim_end();
    let first = title.lines().next().unwrap_or("");
    let opens = has_output || first.len() < title.len();
    let is_open = opens && memory::flag(cx, key, running);
    let mark = if !opens {
        // Keeps the kind under the kinds of the calls that open.
        div().flex_none().size(px(12.0)).into_any_element()
    } else {
        icon(
            if is_open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            12.0,
            theme.colors.secondary,
        )
        .into_any_element()
    };
    let id: SharedString = key.to_owned().into();
    let selector = key.to_owned();
    let mut line = div()
        .id(id.clone())
        .debug_selector(move || selector)
        .flex()
        // The mark and the kind stay at the top of a title that takes several lines.
        .map(|line| {
            if is_open {
                line.items_start()
            } else {
                line.items_center()
            }
        })
        .gap(px(6.0))
        .w_full()
        .text_size(px(theme.text.secondary))
        .text_color(theme.colors.secondary)
        .child(
            div()
                .flex_none()
                .h(px(theme.text.mono * 1.5))
                .flex()
                .items_center()
                .child(mark),
        )
        .child(
            div()
                .flex_none()
                .h(px(theme.text.mono * 1.5))
                .flex()
                .items_center()
                .child(label(name("kind").or(name("name")).unwrap_or("tool"), cx)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(theme.text.mono_family.clone())
                .text_size(px(theme.text.mono))
                .line_height(px(theme.text.mono * 1.5))
                // What runs is read as it goes; what is over steps back.
                .when(running, |title| title.text_color(theme.colors.text))
                .map(|shown| {
                    if is_open {
                        shown.child(shared(title))
                    } else if first.len() < title.len() {
                        shown.truncate().child(shared(&format!("{first} …")))
                    } else {
                        shown.truncate().child(shared(first))
                    }
                }),
        )
        // A call that went well says nothing more: only what runs or went wrong is marked.
        .when(status != "completed", |line| {
            line.child(badge(status, None, cx))
        });
    if opens {
        line = line
            .cursor_pointer()
            .hover(|line| line.text_color(theme.colors.text))
            .on_click(move |_, _, cx| memory::set_flag(cx, &id, !is_open));
    }
    let column = div().flex().flex_col().w_full().child(line);
    if !is_open || !has_output {
        return column;
    }
    let mut body = div().flex().flex_col().gap(px(6.0)).w_full();
    for (index, block) in blocks.iter().enumerate() {
        match text(block, "type") {
            "terminal" => {
                if let Some(found) = find_terminal(text(block, "terminalId")) {
                    body = body.child(terminal(found, cx));
                }
            }
            "diff" => body = body.child(diff(&format!("{key}/diff{index}"), block, cx)),
            _ => {
                let said = content_text(Some(block));
                if !said.is_empty() {
                    body = body.child(code_block(&said, cx));
                }
            }
        }
    }
    let selector = format!("{key}/output");
    column.child(guided(
        body.debug_selector(move || selector)
            .text_color(theme.colors.text),
        cx,
    ))
}

/// What a group of tool calls says: how many, and how many failed or were cancelled (" · M
/// failed", or nothing when none did).
#[must_use]
pub fn tool_group_title(calls: &[&Value]) -> (String, Option<String>) {
    let failed = calls.iter().filter(|call| tool_failed(call)).count();
    (
        format!("{} tool calls", calls.len()),
        (failed > 0).then(|| format!(" · {failed} failed")),
    )
}

/// Finished tool calls that followed one another, as one line: a fold, closed, that opens to
/// its `rows` (each call, and what came between two of them) on a guide line. Whether it is open
/// is remembered under `key`.
pub fn tool_group(
    key: &str,
    calls: &[&Value],
    cx: &App,
    rows: impl FnOnce() -> Vec<AnyElement>,
) -> Div {
    let theme = theme(cx);
    let (count, failed) = tool_group_title(calls);
    let title = div()
        .flex()
        .child(SharedString::from(count))
        .when_some(failed, |title, failed| {
            title.child(
                div()
                    .text_color(theme.colors.bad)
                    .child(SharedString::from(failed)),
            )
        });
    guided_fold(key, title, false, cx, || {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w_full()
            .children(rows())
            .into_any_element()
    })
}

/// One entry of a plan or a todo list, with its mark.
pub(crate) fn plan_entry(content: &str, status: &str, cx: &App) -> Div {
    let theme = theme(cx);
    div()
        .flex()
        .items_start()
        .gap(px(8.0))
        .w_full()
        .child(
            div()
                .pt(px((theme.text.body * theme.text.line_height - 16.0)
                    .max(0.0)
                    / 2.0))
                .child(plan_mark(status, cx)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(status == "completed", |entry| {
                    entry.line_through().text_color(theme.colors.secondary)
                })
                .when(status == "in_progress", |entry| {
                    entry.font_weight(FontWeight::SEMIBOLD)
                })
                .child(shared(content)),
        )
}

/// The agent's plan: a document, or a list of entries with their status (which is also how
/// todos arrive).
pub fn plan(item: &Value, cx: &App) -> Div {
    let mut card = card(false, cx).child(label("Plan", cx));
    match item
        .get("entries")
        .and_then(Value::as_array)
        .filter(|entries| !entries.is_empty())
    {
        Some(entries) => {
            for entry in entries {
                let status = Some(text(entry, "status"))
                    .filter(|status| !status.is_empty())
                    .unwrap_or("pending");
                card = card.child(plan_entry(text(entry, "content"), status, cx));
            }
        }
        None => card = card.child(shared(&content_text(item.get("content")))),
    }
    card
}

/// A subagent: its title and state, and its own session below (`inner`, a nested timeline). Not
/// a card: its timeline hangs from its row on a guide line, so the cards in it are never inside
/// another card.
pub fn subagent(item: &Value, inner: Option<AnyElement>, cx: &App) -> Div {
    let theme = theme(cx);
    let state = item
        .pointer("/state/state")
        .and_then(Value::as_str)
        .unwrap_or("");
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .w_full()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .w_full()
                .child(label("Subagent", cx))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(shared(text(item, "title"))),
                )
                .when(!state.is_empty(), |row| row.child(badge(state, None, cx))),
        )
        .when_some(inner, |column, inner| {
            column.child(
                div()
                    .ml(px(7.0))
                    .pl(px(18.0))
                    .border_l_1()
                    .border_color(theme.colors.edge)
                    .child(inner),
            )
        })
}

/// A label, then a title on the same line.
fn labelled(name: &str, title: &str, size: f32, weight: FontWeight, cx: &App) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(10.0))
        .w_full()
        .child(label(name, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(size))
                .font_weight(weight)
                .child(shared(title)),
        )
}

/// A message between a session and its subagent: the task given, or the result returned.
pub fn session_message(item: &Value, cx: &App) -> Div {
    let theme = theme(cx);
    card(false, cx).child(labelled(
        "Between agents",
        &content_text(item.get("content")),
        theme.text.body,
        FontWeight::NORMAL,
        cx,
    ))
}

/// The agent summarised the conversation to free context: a fold opening to the summary.
pub fn compaction(key: &str, item: &Value, cx: &App) -> Div {
    let title = SharedString::from(format!("Conversation compacted ({})", text(item, "status")));
    fold(key, title, false, cx, || {
        shared(&content_text(item.get("summary"))).into_any_element()
    })
}

/// Called with the id of the option the user chose.
pub type Choose = Rc<dyn Fn(&str, &mut Window, &mut App)>;
/// Called with the form's content, or `None` when the user skips the question.
pub type Answer = Rc<dyn Fn(Option<Value>, &mut Window, &mut App)>;

/// An approval: what the agent wants to do and its options, on a highlighted card while it
/// waits. Once answered it is one quiet line like a tool call: the label, the title and which
/// option was taken; the answer may come from another app.
pub fn approval(key: &str, request: &Request, choose: Option<Choose>, cx: &App) -> Div {
    let theme = theme(cx);
    let options = request
        .params
        .get("options")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if let Some(outcome) = &request.outcome {
        let chosen = outcome
            .pointer("/outcome/optionId")
            .and_then(Value::as_str)
            .unwrap_or("");
        let said =
            if outcome.pointer("/outcome/outcome").and_then(Value::as_str) == Some("cancelled") {
                "Cancelled"
            } else {
                options
                    .iter()
                    .find(|option| text(option, "optionId") == chosen)
                    .map_or(chosen, |option| text(option, "name"))
            };
        let selector = format!("{key}/answered");
        return div()
            .debug_selector(move || selector)
            .flex()
            .items_center()
            .gap(px(6.0))
            .w_full()
            .text_size(px(theme.text.secondary))
            .text_color(theme.colors.secondary)
            // No chevron: its space is left empty, so the title is under the tool calls' titles.
            .child(div().flex_none().size(px(12.0)))
            .child(label("Permission", cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(shared(text(&request.params, "title"))),
            )
            .child(div().flex_none().child(shared(said)));
    }
    let card = card(true, cx).child(labelled(
        "Permission",
        text(&request.params, "title"),
        theme.text.request_title,
        FontWeight::SEMIBOLD,
        cx,
    ));
    let mut row = div().flex().flex_wrap().gap(px(8.0)).pt(px(4.0));
    for (index, option) in options.iter().enumerate() {
        // The first option is the one most often taken.
        let mut one = button(
            format!("{key}/option{index}"),
            text(option, "name"),
            index == 0,
            cx,
        );
        if let Some(choose) = choose.clone() {
            let id = text(option, "optionId").to_owned();
            one = one.on_click(move |_, window, cx| choose(&id, window, cx));
        }
        row = row.child(one);
    }
    card.child(row)
}

/// A question from the agent: a small form built from the schema it sent, with Answer and Skip.
/// Once answered, the values given, or "No answer".
pub fn question(
    key: &str,
    request: &Request,
    answer: Option<Answer>,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let theme = theme(cx);
    let empty = Map::new();
    let fields = request
        .params
        .pointer("/requestedSchema/properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut card = card(request.outcome.is_none(), cx).child(labelled(
        "Question",
        text(&request.params, "message"),
        theme.text.request_title,
        FontWeight::SEMIBOLD,
        cx,
    ));
    if let Some(outcome) = &request.outcome {
        let said = if text(outcome, "action") == "accept" {
            outcome
                .get("content")
                .and_then(Value::as_object)
                .map(|content| {
                    content
                        .values()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        } else {
            "No answer".to_owned()
        };
        return card.child(
            div()
                .text_color(theme.colors.secondary)
                .child(shared(&said)),
        );
    }
    for (name, field) in fields {
        let title = Some(text(field, "title"))
            .filter(|title| !title.is_empty())
            .unwrap_or(name);
        let field_key = format!("{key}/{name}");
        let choices = field
            .get("oneOf")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let control = if choices.is_empty() {
            let title = title.to_owned();
            let made = memory::field(&field_key, window, cx, |window, cx| {
                memory::new_field(window, cx, |_, cx| TextField::new(title, cx))
            });
            div().w_full().child(made)
        } else {
            let group = MenuGroup {
                name: None,
                choices: choices
                    .iter()
                    .map(|choice| MenuChoice {
                        value: text(choice, "const").to_owned(),
                        name: Some(text(choice, "title"))
                            .filter(|name| !name.is_empty())
                            .unwrap_or(text(choice, "const"))
                            .to_owned(),
                        description: None,
                    })
                    .collect(),
            };
            let (remembered, current) = (field_key.clone(), memory::text(cx, &field_key));
            // Not a choice: says that one is still to be made.
            menu(
                &field_key,
                None,
                current.as_deref().unwrap_or("Choose…"),
                &[group],
                Some(Rc::new(move |value, _, cx| {
                    memory::set_text(cx, &remembered, Some(value.to_owned()))
                })),
                cx,
            )
        };
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .w_full()
                .child(
                    div()
                        .text_size(px(theme.text.secondary))
                        .text_color(theme.colors.secondary)
                        .child(shared(title)),
                )
                .child(control),
        );
    }
    let mut send = button(format!("{key}/answer"), "Answer", true, cx);
    let mut skip = button(format!("{key}/skip"), "Skip", false, cx);
    if let Some(answer) = answer {
        let (form, names) = (key.to_owned(), fields.keys().cloned().collect::<Vec<_>>());
        let accept = answer.clone();
        send = send.on_click(move |_, window, cx| {
            let mut content = Map::new();
            for name in &names {
                let field_key = format!("{form}/{name}");
                let typed = memory::text(cx, &field_key).or_else(|| {
                    Some(memory::field(&field_key, window, cx, |window, cx| {
                        memory::new_field(window, cx, |_, cx| TextField::new("", cx))
                    }))
                    .map(|field| field.read(cx).text().to_owned())
                });
                if let Some(value) = typed.filter(|value| !value.is_empty()) {
                    content.insert(name.clone(), json!(value));
                }
            }
            accept(Some(Value::Object(content)), window, cx);
        });
        skip = skip.on_click(move |_, window, cx| answer(None, window, cx));
    }
    card.child(
        div()
            .flex()
            .gap(px(8.0))
            .pt(px(4.0))
            .child(send)
            .child(skip),
    )
}

/// Any kind there is no component for, such as an extension's: a fold titled with the kind,
/// opening to the item's data.
pub fn fallback(key: &str, kind: &str, item: &Value, cx: &App) -> Div {
    fold(key, shared(kind), false, cx, || {
        code_block(&serde_json::to_string_pretty(item).unwrap_or_default(), cx).into_any_element()
    })
}
