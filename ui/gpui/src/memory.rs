//! What the kit remembers between two drawings of a part: which folds are open, what a form
//! holds, which menu is open. Kept as a GPUI global by key, so a row that scrolls out of a long
//! timeline and back comes back as the user left it.
//!
//! Keys are the caller's: a part takes one and derives its own from it. Two parts drawn with the
//! same key share what they remember.

use crate::input::TextField;
use gpui::{App, AppContext as _, Entity, Global, Window};
use std::collections::HashMap;

#[derive(Default)]
struct Memory {
    flags: HashMap<String, bool>,
    texts: HashMap<String, String>,
    fields: HashMap<String, Entity<TextField>>,
    menu: Option<String>,
}

impl Global for Memory {}

/// Whether `key` is set; `default` until it was.
#[must_use]
pub fn flag(cx: &App, key: &str, default: bool) -> bool {
    cx.try_global::<Memory>()
        .and_then(|memory| memory.flags.get(key).copied())
        .unwrap_or(default)
}

pub fn set_flag(cx: &mut App, key: &str, value: bool) {
    cx.default_global::<Memory>()
        .flags
        .insert(key.to_owned(), value);
    cx.refresh_windows();
}

/// The text kept under `key`, such as a form's choice.
#[must_use]
pub fn text(cx: &App, key: &str) -> Option<String> {
    cx.try_global::<Memory>()
        .and_then(|memory| memory.texts.get(key).cloned())
}

pub fn set_text(cx: &mut App, key: &str, value: Option<String>) {
    let texts = &mut cx.default_global::<Memory>().texts;
    match value {
        Some(value) => texts.insert(key.to_owned(), value),
        None => texts.remove(key),
    };
    cx.refresh_windows();
}

/// The text field kept under `key`, made by `make` the first time.
pub fn field(
    key: &str,
    window: &mut Window,
    cx: &mut App,
    make: impl FnOnce(&mut Window, &mut App) -> Entity<TextField>,
) -> Entity<TextField> {
    if let Some(found) = cx
        .try_global::<Memory>()
        .and_then(|memory| memory.fields.get(key))
    {
        return found.clone();
    }
    let made = make(window, cx);
    // A field draws itself, but what is around it (a Save button, a menu) may depend on its text.
    cx.observe(&made, |_, cx| cx.refresh_windows()).detach();
    cx.default_global::<Memory>()
        .fields
        .insert(key.to_owned(), made.clone());
    made
}

/// The key of the menu that is open, if one is. Only one is open at a time.
#[must_use]
pub fn open_menu(cx: &App) -> Option<String> {
    cx.try_global::<Memory>()
        .and_then(|memory| memory.menu.clone())
}

pub fn set_open_menu(cx: &mut App, key: Option<String>) {
    cx.default_global::<Memory>().menu = key;
    cx.refresh_windows();
}

/// Forgets everything kept under keys that start with `prefix`: call it when what they belonged
/// to is gone, such as a session that was closed.
pub fn forget(cx: &mut App, prefix: &str) {
    let memory = cx.default_global::<Memory>();
    memory.flags.retain(|key, _| !key.starts_with(prefix));
    memory.texts.retain(|key, _| !key.starts_with(prefix));
    memory.fields.retain(|key, _| !key.starts_with(prefix));
    if memory
        .menu
        .as_ref()
        .is_some_and(|key| key.starts_with(prefix))
    {
        memory.menu = None;
    }
}

/// A new entity, for the closures of [`field`].
pub fn new_field(
    window: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut gpui::Context<TextField>) -> TextField,
) -> Entity<TextField> {
    cx.new(|cx| build(window, cx))
}
