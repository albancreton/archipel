//! Components that draw an archipel session with [GPUI](https://www.gpui.rs): the third set built
//! to `ui/COMPONENTS.md`, after the React and SwiftUI kits.
//!
//! A component draws what the client holds (`archipel_core::state`). It never talks to the
//! daemon itself: what the user does goes out through callbacks the app gives.
//!
//! ```ignore
//! archipel_gpui::init(cx);                                   // keys for text fields, the theme
//! let chat = cx.new(|cx| ChatView::new(conversation, actions, window, cx));
//! chat.update(cx, |chat, cx| chat.set_conversation(newer, cx)); // after each change
//! ```
//!
//! [`ChatView`], [`Composer`] and [`SessionList`] are entities, since they hold what the user is
//! doing. Everything else is a function that returns an element: [`items`], [`status`],
//! [`parts`], [`settings`], [`markdown`], [`diff`].

pub mod ansi;
pub mod chat;
pub mod composer;
pub mod diff;
pub mod input;
pub mod items;
pub mod markdown;
pub mod memory;
pub mod parts;
pub mod sessions;
pub mod settings;
pub mod status;
pub mod theme;

pub use chat::{
    ChatActions, ChatView, ItemViews, TimelineRow, timeline, timeline_row, timeline_row_of,
    timeline_rows,
};
pub use composer::{Accepts, Attachment, Command, Composer, Mention};
pub use input::{FieldEvent, TextField};
pub use sessions::{SessionList, SessionListEvent, SessionSummary, session_row};
pub use settings::{Setting, SettingValue, settings_bar};
pub use status::{OpenElsewhere, Outcome};
pub use theme::{Theme, Tone, set_theme, theme};

use gpui::{App, WindowAppearance};

/// Call once when the app starts: binds the keys of the kit's text fields, and sets the
/// standard theme (dark or light as the system is) unless the app already set one.
pub fn init(cx: &mut App) {
    input::bind_keys(cx);
    if !cx.has_global::<Theme>() {
        let dark = matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        cx.set_global(Theme::standard(dark));
    }
}
