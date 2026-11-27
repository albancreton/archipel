# GPUI component kit

`archipel-gpui`: the components of `ui/COMPONENTS.md` for [GPUI](https://www.gpui.rs), the UI framework of the Zed editor. Optional, like the other kits. It draws what the client holds (`archipel_core::state`) and never talks to the daemon; `archipel-gpui-live` (in `live/`) is the other half for an app that wants that done too.

This folder is a Cargo workspace of its own, apart from the repository's: GPUI is a large dependency with its own system requirements, and the daemon's build should not carry it. Run `cargo` from here.

```rust
archipel_gpui::init(cx);                                        // once: keys for text fields, the standard theme

// From plain state, with your own closures:
let chat = cx.new(|cx| ChatView::new(conversation, actions, window, cx));
chat.update(cx, |chat, cx| chat.set_conversation(newer, cx));   // after each change

// Or on a session of a running daemon (archipel-gpui-live):
let client = Client::connect(Endpoint::Local(socket), "my-app", cx);
let session = client.update(cx, |client, cx| client.session(id, cx));
let chat = live_chat(&session, None, window, cx);               // draws and acts on it
```

- **What is an entity and what is a function.** `ChatView`, `Composer` and `SessionList` are entities, since they hold what the user is doing. Everything else is a function that returns an element: `items::tool_call(key, &item, running, …)`, `status::status_bar(&state, cx)`, `settings_bar(…)`, `diff::diff(key, &block, cx)`, the shared `parts`. Call them from a view's `render`: GPUI gives ids and tooltips a view to belong to.
- **Keys.** A function that can be folded or filled in takes a `key`, such as `"<session id>/<item id>"`. The kit remembers under it which folds are open, what a form holds and which menu is open (`memory`), so a row that scrolls out of a long timeline comes back as the user left it. `chat::forget(session_id, cx)` drops what was remembered for a session.
- **Actions** are closures in `ChatActions`. One that can fail (`prompt`, `set_option`, `fork`) returns a `Task<Result<(), String>>`: the control waits for it, and says the reason when it fails. Leave one out and its control is not drawn.
- **Text fields.** GPUI has none, so the kit draws its own (`TextField`): a caret, a selection, the clipboard, the system's input methods, and more than one line in the composer (Shift+Enter breaks the line, Enter sends). `init` binds its keys under the key context `ArchipelField`.
- **The agent's text is drawn as Markdown** (on `pulldown-cmark`), as GPUI elements: headings, lists, tables, quotes, code blocks. Markup the agent writes is shown as text, links go only to `http`, `https` and `mailto`, images are named and not fetched, and an open code fence in text that is still arriving is code. There is no code highlighting.
- **Tool calls** are one quiet line each, not a card: `items::tool_call` shows what a call produced while it runs (`running`, from `chat::tool_runs` in a timeline, `items::tool_running(&item)` for a call on its own) and folds it away when it is over; pressing the line opens and closes it, and the choice is remembered under the call's key. Finished calls in a row are one `items::tool_group` ("N tool calls"), with the thoughts between them and the answered approvals after them. `timeline_rows(&state, &inline)` says which rows a timeline has, and `timeline_row_of` draws one; an answered approval is a quiet line too.
- **Long sessions:** `ChatView` draws the timeline with GPUI's `list`, anchored at the newest item, so only the rows on screen are built. The rows (entries and groups) are worked out once per change of state. `timeline(…)` draws every row, for a short one of your own (a subagent's session is drawn with it, inside its item).
- **Composer:** typing `/` lists the agent's commands; with `set_mentions` (a function of yours that finds files) typing `@` before the caret lists them; with `set_accepts` it offers "Attach" (the system's file dialog) and takes dropped files. Arrows, Tab, Enter and Escape work in the lists. Files cannot be pasted.
- **Session list:** grouped under each folder's name. A row says `needs you`, `running`, `open elsewhere`, and `from <agent>` for a session started outside the app. Rename and delete are buttons on the row under the pointer, only where the agent can do them; rename is a field in the row, and delete asks in the row first. It emits `SessionListEvent::Opened`.
- **Themes:** one `Theme` is the app's, a GPUI global: `set_theme(cx, theme)` restyles every part. `Theme::standard(dark)` has the colours of the React kit's stylesheet; `init` picks light or dark as the system is. The Minitel theme is `archipel-gpui-minitel` (`ui/themes/minitel/gpui`): `set_theme(cx, minitel(true))`. A theme is a value, so an app can change one colour of it.
- `ItemViews` maps a target such as `x.acme.checkpoint` to a function of yours that draws an extension's kind.

**Following a daemon (`live/`).** `Client` holds the connection (on a thread of its own, since the Rust client is written for tokio), the session list and the agents, and connects again when the connection is lost. `Session` stays in step with the daemon through `archipel_core::mirror::Mirror`, the same reference the other SDKs are tested against, and continues from the last sequence number after a reconnect. `actions(&session, on_fork)` gives the `ChatActions` that act on it, and `live_chat` a `ChatView` that follows it. `cargo run -p archipel-gpui-live --example chat -- --home DIR` opens a chat on a new session.

**Catalogue.** `cargo run -p archipel-gpui-catalog` opens every component in each of its states in a window, with the same data and sections as the React and SwiftUI catalogues; `-- --theme minitel` and `--light` choose the look, which can also be changed in the window. It is a window only: GPUI 0.2 cannot draw off screen, so there is no page of pictures as the other kits have.

**Tests.** `cargo test --workspace` (after `cargo build` at the repository's root, for the daemon):

- the parts that are plain logic: reading ANSI codes, patches, Markdown, settings, commands and mentions, times;
- `tests/kit.rs` draws every recorded session of `testdata/transcripts` and uses the kit as a person would, in GPUI's test windows: it presses folds, tool calls and their groups, approval buttons, menus and session rows, checks which calls are grouped, and types into the question form and the composer;
- `live/tests/daemon.rs` drives a whole turn through a `ChatView` against a daemon with the fake agent: the message is typed and sent, the approval pressed, the turn followed to its end.

GPUI's test windows lay out and take input but draw nothing, so no test looks at pixels.

**Linux.** The kit has no platform code of its own, but it was built and run on macOS only. On Linux, GPUI 0.2.2 from crates.io did not build when this was written: one of its dependencies (`xattr` 0.2.3) does not compile against the current `libc`.
