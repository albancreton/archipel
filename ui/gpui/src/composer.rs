//! Where the user writes to the agent: text, attachments, the agent's commands after `/`, and
//! files after `@`.

use crate::input::{self, FieldEvent, TextField};
use crate::items::{shared, text};
use crate::parts::{Handler, Icon, button, icon, problem};
use crate::status::Outcome;
use crate::theme::theme;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{
    App, Context, Entity, ExternalPaths, Image, ImageFormat, IntoElement, PathPromptOptions,
    Render, Subscription, Task, Window, div, img, prelude::*, px,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// A slash command the agent offers (`state.values["availableCommands"]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    /// What to type after the command, when the agent says.
    pub hint: Option<String>,
}

impl Command {
    /// Reads the agent's `availableCommands`.
    #[must_use]
    pub fn all(available: Option<&Value>) -> Vec<Self> {
        let optional = |text: &str| {
            Some(text)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        available
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        Some(Self {
                            name: optional(text(entry, "name"))?,
                            description: optional(text(entry, "description")),
                            hint: entry
                                .pointer("/input/hint")
                                .and_then(Value::as_str)
                                .and_then(optional),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The commands for what was typed after `/`: those whose name starts with it first, then
    /// those that contain it.
    #[must_use]
    pub fn matching<'a>(query: &str, commands: &'a [Self]) -> Vec<&'a Self> {
        let wanted = query.to_lowercase();
        let starts = |command: &&Self| command.name.to_lowercase().starts_with(&wanted);
        let contains =
            |command: &&Self| !starts(command) && command.name.to_lowercase().contains(&wanted);
        commands
            .iter()
            .filter(starts)
            .chain(commands.iter().filter(contains))
            .collect()
    }
}

/// Something the user can mention with `@`: a file, usually. The app supplies them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    /// Where it is, such as `file:///work/app/src/main.ts`. Sent to the agent as a link.
    pub uri: String,
    /// What is written into the text after `@`, such as `src/main.ts`. No spaces.
    pub name: String,
    /// A second line in the menu.
    pub detail: Option<String>,
}

/// A file the user attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub name: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

impl Attachment {
    #[must_use]
    pub fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }
}

/// What the agent takes in a message besides text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Accepts {
    /// Pictures.
    pub image: bool,
    /// Other files, sent with their content.
    pub files: bool,
}

impl Accepts {
    /// From one entry of the client's agents. Nothing until the agent has started.
    #[must_use]
    pub fn of(agent: Option<&Value>) -> Self {
        let has = |path: &str| {
            agent
                .and_then(|agent| agent.pointer(path))
                .is_some_and(|value| !value.is_null())
        };
        Self {
            image: has("/capabilities/session/prompt/image"),
            files: has("/capabilities/session/prompt/embeddedContext"),
        }
    }
}

/// The command the user is typing: the draft is `/` and a name with nothing after it yet.
#[must_use]
pub fn command_query(draft: &str) -> Option<&str> {
    let name = draft.strip_prefix('/')?;
    (!name.contains(|character: char| character.is_whitespace() || character == '/'))
        .then_some(name)
}

/// The mention the user is typing just before the caret: `@` at the start of a word, and what
/// follows it. Gives where the `@` is and what was typed after it.
#[must_use]
pub fn mention_query(draft: &str, caret: usize) -> Option<(usize, &str)> {
    let before = draft.get(..caret)?;
    let at = before.rfind('@')?;
    let query = &before[at + 1..];
    if query.contains(char::is_whitespace) {
        return None;
    }
    if before[..at]
        .chars()
        .next_back()
        .is_some_and(|previous| !previous.is_whitespace())
    {
        return None;
    }
    Some((at, query))
}

/// The kind of a file, from its name. Enough to tell pictures and text from the rest.
#[must_use]
pub fn mime_type(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension)
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "sh" => "application/x-sh",
        "js" | "mjs" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "html" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "md" => "text/markdown",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" | "log" | "rs" | "py" | "go" | "c" | "h" | "cpp" | "java" | "swift" | "rb" | "kt"
        | "lock" | "ini" | "cfg" | "conf" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn percent_encoded(name: &str) -> String {
    name.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// What goes to the agent, as content blocks: the text, a `resource_link` for each mention
/// whose `@name` is still in the text, an `image` for each picture and a `resource` with its
/// content for each other file.
#[must_use]
pub fn prompt_content(draft: &str, attachments: &[Attachment], mentions: &[Mention]) -> Vec<Value> {
    let mut blocks = Vec::new();
    if !draft.trim().is_empty() {
        blocks.push(json!({ "type": "text", "text": draft }));
    }
    let mut linked: Vec<&str> = Vec::new();
    for mention in mentions {
        let written = draft
            .split_whitespace()
            .any(|word| word.strip_prefix('@') == Some(mention.name.as_str()));
        if written && !linked.contains(&mention.uri.as_str()) {
            linked.push(&mention.uri);
            blocks
                .push(json!({ "type": "resource_link", "uri": mention.uri, "name": mention.name }));
        }
    }
    for file in attachments {
        if file.is_image() {
            blocks.push(json!({ "type": "image", "mimeType": file.mime_type, "data": BASE64.encode(&file.data) }));
            continue;
        }
        let uri = format!("file:///{}", percent_encoded(&file.name));
        let textual = file.mime_type.starts_with("text/")
            || [
                "json",
                "xml",
                "javascript",
                "typescript",
                "yaml",
                "toml",
                "x-sh",
            ]
            .iter()
            .any(|kind| file.mime_type.contains(kind));
        let resource = match std::str::from_utf8(&file.data) {
            Ok(text) if textual => json!({ "uri": uri, "mimeType": file.mime_type, "text": text }),
            _ => {
                json!({ "uri": uri, "mimeType": file.mime_type, "blob": BASE64.encode(&file.data) })
            }
        };
        blocks.push(json!({ "type": "resource", "resource": resource }));
    }
    blocks
}

/// Sends the message: its content blocks. If it fails, the composer gives everything back.
pub type Send = Rc<dyn Fn(Vec<Value>, &mut Window, &mut App) -> Outcome>;
/// Finds what the user may mention, for what they typed after `@`.
pub type FindMentions = Rc<dyn Fn(&str, &mut App) -> Task<Vec<Mention>>>;

struct Entry {
    label: String,
    detail: String,
    /// What the draft becomes from `from` to the caret.
    insert: String,
    from: usize,
    mention: Option<Mention>,
}

pub struct Composer {
    field: Entity<TextField>,
    running: bool,
    send: Send,
    cancel: Option<Handler>,
    commands: Vec<Command>,
    mentions: Option<FindMentions>,
    accepts: Accepts,
    /// The largest file that can be attached, in bytes.
    max_bytes: usize,
    attached: Vec<Attachment>,
    mentioned: Vec<Mention>,
    found: Vec<Mention>,
    finding: Option<Task<()>>,
    chosen: usize,
    closed: bool,
    problem: Option<String>,
    _watch: Subscription,
}

impl Composer {
    pub fn new(send: Send, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new("Message the agent…", cx).multiline());
        let watch = cx.subscribe_in(
            &field,
            window,
            |composer, _, event, window, cx| match event {
                FieldEvent::Changed => composer.changed(cx),
                FieldEvent::Submitted => {
                    if !composer.pick_chosen(cx) {
                        composer.submit(window, cx);
                    }
                }
                FieldEvent::Cancelled => {}
            },
        );
        Self {
            field,
            running: false,
            send,
            cancel: None,
            commands: Vec::new(),
            mentions: None,
            accepts: Accepts::default(),
            max_bytes: 5_000_000,
            attached: Vec::new(),
            mentioned: Vec::new(),
            found: Vec::new(),
            finding: None,
            chosen: 0,
            closed: false,
            problem: None,
            _watch: watch,
        }
    }

    /// The field, to read or set a draft the app keeps, or to focus it.
    #[must_use]
    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// A turn is running: the placeholder says so and Stop is offered (with `cancel`).
    pub fn set_running(&mut self, running: bool, cx: &mut Context<Self>) {
        if self.running != running {
            self.running = running;
            let placeholder = if running {
                "Add to the running turn…"
            } else {
                "Message the agent…"
            };
            self.field
                .update(cx, |field, cx| field.set_placeholder(placeholder, cx));
            cx.notify();
        }
    }

    pub fn set_send(&mut self, send: Send) {
        self.send = send;
    }

    pub fn set_cancel(&mut self, cancel: Option<Handler>, cx: &mut Context<Self>) {
        self.cancel = cancel;
        cx.notify();
    }

    pub fn set_commands(&mut self, commands: Vec<Command>, cx: &mut Context<Self>) {
        if self.commands != commands {
            self.commands = commands;
            cx.notify();
        }
    }

    /// Without it `@` is only a character.
    pub fn set_mentions(&mut self, mentions: Option<FindMentions>, cx: &mut Context<Self>) {
        self.mentions = mentions;
        cx.notify();
    }

    pub fn set_accepts(&mut self, accepts: Accepts, cx: &mut Context<Self>) {
        if self.accepts != accepts {
            self.accepts = accepts;
            cx.notify();
        }
    }

    pub fn set_max_bytes(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes;
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.closed = false;
        self.chosen = 0;
        let query = {
            let field = self.field.read(cx);
            mention_query(field.text(), field.caret()).map(|(_, query)| query.to_owned())
        };
        match (self.mentions.clone(), query) {
            (Some(find), Some(query)) => {
                let search = find(&query, cx);
                // Only the newest search counts: the one before is dropped with its task.
                self.finding = Some(cx.spawn(async move |composer, cx| {
                    let found = search.await;
                    composer
                        .update(cx, |composer, cx| {
                            composer.found = found;
                            cx.notify();
                        })
                        .ok();
                }));
            }
            _ => {
                self.finding = None;
                self.found.clear();
            }
        }
        cx.notify();
    }

    /// The menu for what is being typed: commands after `/`, or files after `@`.
    fn menu(&self, cx: &App) -> Vec<Entry> {
        if self.closed {
            return Vec::new();
        }
        let field = self.field.read(cx);
        let (draft, caret) = (field.text(), field.caret());
        if let Some(query) = command_query(draft) {
            return Command::matching(query, &self.commands)
                .into_iter()
                .take(8)
                .map(|command| Entry {
                    label: format!("/{}", command.name),
                    detail: [command.hint.as_deref(), command.description.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · "),
                    insert: format!("/{} ", command.name),
                    from: 0,
                    mention: None,
                })
                .collect();
        }
        if self.mentions.is_some()
            && let Some((from, _)) = mention_query(draft, caret)
        {
            return self
                .found
                .iter()
                .take(8)
                .map(|mention| Entry {
                    label: format!("@{}", mention.name),
                    detail: mention.detail.clone().unwrap_or_default(),
                    insert: format!("@{} ", mention.name),
                    from,
                    mention: Some(mention.clone()),
                })
                .collect();
        }
        Vec::new()
    }

    fn pick(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.mentioned.extend(entry.mention);
        self.field.update(cx, |field, cx| {
            let (draft, caret) = (field.text().to_owned(), field.caret());
            let from = entry.from.min(caret);
            // A command replaces the whole draft; a mention, what was typed from its `@`.
            let rest = if entry.from == 0 && draft.starts_with('/') {
                ""
            } else {
                &draft[caret..]
            };
            field.set_text(format!("{}{}{rest}", &draft[..from], entry.insert), cx);
        });
        cx.notify();
    }

    /// Takes the chosen entry of the menu, if a menu is open.
    fn pick_chosen(&mut self, cx: &mut Context<Self>) -> bool {
        let mut menu = self.menu(cx);
        if menu.is_empty() {
            return false;
        }
        let at = self.chosen.min(menu.len() - 1);
        self.pick(menu.swap_remove(at), cx);
        true
    }

    fn move_chosen(&mut self, by: isize, cx: &mut Context<Self>) -> bool {
        let count = self.menu(cx).len() as isize;
        if count == 0 {
            return false;
        }
        self.chosen =
            ((self.chosen.min(count as usize - 1) as isize + by + count) % count) as usize;
        cx.notify();
        true
    }

    /// Attaches files by their paths: what the picker gave, or what was dropped.
    pub fn attach(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.problem = None;
        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mime_type = mime_type(&name);
            let image = mime_type.starts_with("image/");
            if !(if image {
                self.accepts.image
            } else {
                self.accepts.files
            }) {
                self.problem = Some(format!(
                    "{name}: the agent does not take {}",
                    if image { "pictures" } else { "files" }
                ));
                continue;
            }
            let Ok(data) = std::fs::read(path) else {
                self.problem = Some(format!("{name}: could not be read"));
                continue;
            };
            if data.len() > self.max_bytes {
                self.problem = Some(format!(
                    "{name}: larger than {} MB",
                    self.max_bytes / 1_000_000
                ));
                continue;
            }
            self.attached.push(Attachment {
                name,
                mime_type: mime_type.to_owned(),
                data,
            });
        }
        cx.notify();
    }

    fn choose_files(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |composer, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                composer
                    .update(cx, |composer, cx| composer.attach(&paths, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Sends what the field holds. A refused message goes back into the field, unless the user
    /// has typed since.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.field.read(cx).text().to_owned();
        let (files, links) = (
            std::mem::take(&mut self.attached),
            std::mem::take(&mut self.mentioned),
        );
        let content = prompt_content(&draft, &files, &links);
        if content.is_empty() {
            (self.attached, self.mentioned) = (files, links);
            return;
        }
        self.field.update(cx, |field, cx| field.set_text("", cx));
        self.problem = None;
        let work = (self.send)(content, window, cx);
        cx.spawn(async move |composer, cx| {
            let Err(why) = work.await else { return };
            composer
                .update(cx, |composer, cx| {
                    if composer.field.read(cx).text().is_empty() {
                        composer
                            .field
                            .update(cx, |field, cx| field.set_text(draft, cx));
                    }
                    if composer.attached.is_empty() {
                        composer.attached = files;
                    }
                    if composer.mentioned.is_empty() {
                        composer.mentioned = links;
                    }
                    // The notice above the composer already says it when the session is open elsewhere.
                    if !why.is_empty() {
                        composer.problem = Some(format!("Not sent: {why}"));
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
        cx.notify();
    }
}

fn thumbnail(file: &Attachment) -> Option<Arc<Image>> {
    let format = match file.mime_type.as_str() {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, file.data.clone())))
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        let menu = self.menu(cx);
        let at = self.chosen.min(menu.len().saturating_sub(1));
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .w_full()
            .text_color(theme.colors.text)
            // Asked before the field: while a menu is open these keys are the menu's.
            .capture_action(cx.listener(|composer, _: &input::Up, _, cx| {
                if composer.move_chosen(-1, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|composer, _: &input::Down, _, cx| {
                if composer.move_chosen(1, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|composer, _: &input::Complete, _, cx| {
                if composer.pick_chosen(cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|composer, _: &input::Cancel, _, cx| {
                if !composer.menu(cx).is_empty() {
                    composer.closed = true;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_drop(cx.listener(|composer, paths: &ExternalPaths, _, cx| {
                if composer.accepts.image || composer.accepts.files {
                    composer.attach(paths.paths(), cx);
                }
            }));
        if !menu.is_empty() {
            let mut list = div()
                .flex()
                .flex_col()
                .w_full()
                .p(px(4.0))
                .rounded(px(theme.card_radius))
                .bg(theme.colors.raised)
                .border_1()
                .border_color(theme.colors.edge);
            for (index, entry) in menu.into_iter().enumerate() {
                let chosen = index == at;
                list = list.child(
                    div()
                        .id(("entry", index))
                        .flex()
                        .items_baseline()
                        .gap(px(10.0))
                        .w_full()
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(theme.control_radius))
                        .cursor_pointer()
                        .when(chosen, |row| row.bg(theme.colors.accent.opacity(0.22)))
                        .child(
                            div()
                                .flex_none()
                                .font_family(theme.text.mono_family.clone())
                                .text_size(px(theme.text.mono))
                                .child(shared(&entry.label)),
                        )
                        // On the chosen row the detail takes the text colour, to stay readable on its fill.
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(theme.text.meta))
                                .text_color(if chosen {
                                    theme.colors.text
                                } else {
                                    theme.colors.secondary
                                })
                                .child(shared(&entry.detail)),
                        )
                        .on_click(cx.listener(move |composer, _, _, cx| {
                            let mut menu = composer.menu(cx);
                            if index < menu.len() {
                                composer.pick(menu.swap_remove(index), cx);
                            }
                        })),
                );
            }
            column = column.child(list);
        }
        if !self.attached.is_empty() {
            let mut row = div().flex().flex_wrap().gap(px(6.0));
            for (index, file) in self.attached.iter().enumerate() {
                row = row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .max_w(px(320.0))
                        .pl(px(8.0))
                        .pr(px(4.0))
                        .py(px(3.0))
                        .rounded(px(theme.control_radius))
                        .bg(theme.colors.raised)
                        .text_size(px(theme.text.meta))
                        .when_some(thumbnail(file), |chip, picture| {
                            chip.child(img(picture).size(px(20.0)).rounded(px(3.0)))
                        })
                        .child(div().min_w_0().truncate().child(shared(&file.name)))
                        .child(
                            div()
                                .id(("remove", index))
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .size(px(20.0))
                                .rounded(px(4.0))
                                .cursor_pointer()
                                .hover(|remove| remove.bg(theme.colors.line))
                                .child(icon(Icon::Close, 12.0, theme.colors.secondary))
                                .tooltip(crate::parts::tip(&format!("Remove {}", file.name)))
                                .on_click(cx.listener(move |composer, _, _, cx| {
                                    if index < composer.attached.len() {
                                        composer.attached.remove(index);
                                        cx.notify();
                                    }
                                })),
                        ),
                );
            }
            column = column.child(row);
        }
        if let Some(why) = &self.problem {
            column = column.child(problem(why, cx));
        }
        let mut row = div()
            .flex()
            .items_end()
            .gap(px(8.0))
            .w_full()
            .child(div().flex_1().min_w_0().child(self.field.clone()));
        if self.accepts.image || self.accepts.files {
            row = row.child(button("attach", "Attach", false, cx).on_click(
                cx.listener(|composer, _, window, cx| composer.choose_files(window, cx)),
            ));
        }
        row = row.child(
            button("send", "Send", true, cx)
                .on_click(cx.listener(|composer, _, window, cx| composer.submit(window, cx))),
        );
        if self.running
            && let Some(cancel) = self.cancel.clone()
        {
            row = row.child(
                button("stop", "Stop", false, cx).on_click(move |_, window, cx| cancel(window, cx)),
            );
        }
        column.child(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_mentions_are_found_in_the_draft() {
        assert_eq!(command_query("/he"), Some("he"));
        assert_eq!(command_query("/help me"), None);
        assert_eq!(command_query("say /help"), None);
        assert_eq!(mention_query("look at @src/ma", 15), Some((8, "src/ma")));
        assert_eq!(mention_query("mail a@b", 8), None);
        assert_eq!(mention_query("@a b", 4), None);
        assert_eq!(mention_query("@a b", 2), Some((0, "a")));
    }

    #[test]
    fn matching_commands_put_those_that_start_first() {
        let commands = Command::all(Some(
            &json!([{ "name": "recompact" }, { "name": "compact", "input": { "hint": "what to keep" } }, { "name": "help" }]),
        ));
        let names: Vec<_> = Command::matching("comp", &commands)
            .iter()
            .map(|command| command.name.as_str())
            .collect();
        assert_eq!(names, ["compact", "recompact"]);
        assert_eq!(commands[1].hint.as_deref(), Some("what to keep"));
    }

    #[test]
    fn a_prompt_carries_its_text_mentions_and_files() {
        let mentions = [
            Mention {
                uri: "file:///a.rs".into(),
                name: "a.rs".into(),
                detail: None,
            },
            Mention {
                uri: "file:///gone.rs".into(),
                name: "gone.rs".into(),
                detail: None,
            },
        ];
        let files = [
            Attachment {
                name: "shot.png".into(),
                mime_type: "image/png".into(),
                data: vec![1, 2],
            },
            Attachment {
                name: "my notes.txt".into(),
                mime_type: "text/plain".into(),
                data: b"hi".to_vec(),
            },
        ];
        let blocks = prompt_content("see @a.rs", &files, &mentions);
        assert_eq!(blocks.len(), 4);
        assert_eq!(
            blocks[1],
            json!({ "type": "resource_link", "uri": "file:///a.rs", "name": "a.rs" })
        );
        assert_eq!(blocks[2]["type"], "image");
        assert_eq!(
            blocks[3]["resource"],
            json!({ "uri": "file:///my%20notes.txt", "mimeType": "text/plain", "text": "hi" })
        );
        assert!(prompt_content("  ", &[], &[]).is_empty());
    }
}
