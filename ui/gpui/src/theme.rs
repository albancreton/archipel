//! How the kit looks: its colours, type and shapes. One [`Theme`] is the app's, kept as a GPUI
//! global; every part reads it when it is drawn, so setting another restyles them all.
//!
//! [`Theme::standard`] is the kit's own plain look, the same colours as the React kit's
//! stylesheet. A theme such as Minitel (`ui/themes/minitel/gpui`) is another value of this type.

use gpui::{App, FontWeight, Global, Hsla, Rgba, SharedString, rgb};

/// A colour from its `0xRRGGBB` value.
#[must_use]
pub fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// `amount` of one colour and the rest of the other, mixed in sRGB.
#[must_use]
pub fn mix(one: Hsla, other: Hsla, amount: f32) -> Hsla {
    let (a, b) = (Rgba::from(one), Rgba::from(other));
    let between = |x: f32, y: f32| x * amount + y * (1.0 - amount);
    Rgba {
        r: between(a.r, b.r),
        g: between(a.g, b.g),
        b: between(a.b, b.b),
        a: between(a.a, b.a),
    }
    .into()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    /// Text, and secondary text (labels, metadata, what is done).
    pub text: Hsla,
    pub secondary: Hsla,
    /// Behind the chat and the session list.
    pub background: Hsla,
    /// Cards: plans, requests, messages between agents.
    pub card: Hsla,
    /// Chips, menus, buttons that are not the primary one.
    pub raised: Hsla,
    /// The user's message, and the text on it.
    pub message: Hsla,
    pub on_message: Hsla,
    /// Code, diffs, terminal output, fields.
    pub sunken: Hsla,
    /// Borders, and the stronger edge of a hovered border, a meter, a guide line.
    pub line: Hsla,
    pub edge: Hsla,
    /// Primary buttons and links, with `on_accent` on them.
    pub accent: Hsla,
    pub on_accent: Hsla,
    /// Done, failed, running, and waiting for the user.
    pub ok: Hsla,
    pub bad: Hsla,
    pub running: Hsla,
    pub attention: Hsla,
    /// The terminal's 16 colours, for command output.
    pub ansi: [Hsla; 16],
}

/// Text sizes, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub body: f32,
    /// What the agent said: the largest text of a turn, never under `body`.
    pub answer: f32,
    /// Folds, notices, the activity line.
    pub secondary: f32,
    /// The session's title, and a request's.
    pub title: f32,
    pub request_title: f32,
    /// The agent's headings: level 1, then the others.
    pub heading_large: f32,
    pub heading: f32,
    /// Tool titles and commands, and code blocks, diffs and terminal output.
    pub mono: f32,
    pub code: f32,
    /// Badges, a row's details, usage.
    pub meta: f32,
    /// The uppercase word naming a part.
    pub label: f32,
    pub label_weight: FontWeight,
    /// A line's height, as a multiple of the text's size.
    pub line_height: f32,
    /// The family of the text; `None` leaves the window's own.
    pub family: Option<SharedString>,
    /// The family of code, commands and terminal output.
    pub mono_family: SharedString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeStyle {
    /// A coloured outline around coloured text.
    Outlined,
    /// A pale fill of the tone, a dot of the tone, and text between the tone and the text colour.
    Tinted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub colors: Colors,
    pub text: Type,
    pub badge: BadgeStyle,
    pub card_radius: f32,
    /// Code blocks, chips, fields and buttons.
    pub control_radius: f32,
    /// A card's padding: across, then up and down.
    pub card_padding: (f32, f32),
    /// Between the rows of a timeline.
    pub row_spacing: f32,
    /// The border of a card waiting for the user, and how much of the attention colour fills it.
    pub waiting_border: f32,
    pub waiting_fill: f32,
    /// The height of buttons, fields and menus.
    pub control_height: f32,
    /// True: the session list's folder headings are drawn as labels.
    pub folder_labels: bool,
    pub dark: bool,
}

impl Global for Theme {}

/// The family code is set in, when a theme does not choose one.
#[must_use]
pub fn default_mono_family() -> SharedString {
    if cfg!(target_os = "macos") {
        "Menlo".into()
    } else if cfg!(target_os = "windows") {
        "Consolas".into()
    } else {
        "DejaVu Sans Mono".into()
    }
}

impl Theme {
    /// The kit's own look, light or dark: the colours of the React kit's stylesheet.
    #[must_use]
    pub fn standard(dark: bool) -> Self {
        let pick = |on_dark: u32, on_light: u32| hex(if dark { on_dark } else { on_light });
        let ansi: [u32; 16] = if dark {
            [
                0x3B3B42, 0xFF7B72, 0x4CC38A, 0xE3B341, 0x6EA0FF, 0xD2A8FF, 0x56D4DD, 0xC9C9D1,
                0x7D7D88, 0xFFA198, 0x7EE2A8, 0xF2CC60, 0x9EC1FF, 0xE2C5FF, 0x87E6EE, 0xFFFFFF,
            ]
        } else {
            [
                0x1C1C1E, 0xC2362B, 0x1A7F4B, 0x9A6700, 0x2F6FEB, 0x8250DF, 0x1B7C83, 0x6B6B70,
                0x4D4D52, 0xD1453B, 0x2A9D63, 0xB07D00, 0x4A84FF, 0x9A6BF0, 0x2A97A0, 0x9A9AA3,
            ]
        };
        let running = pick(0xE3B341, 0x9A6700);
        Self {
            colors: Colors {
                text: pick(0xECECF1, 0x1C1C1E),
                secondary: pick(0x9A9AA3, 0x6B6B70),
                background: pick(0x1B1B1F, 0xFFFFFF),
                card: pick(0x26262C, 0xF4F4F6),
                raised: pick(0x303038, 0xEAEAEE),
                message: pick(0x6EA0FF, 0x2F6FEB),
                on_message: pick(0x101014, 0xFFFFFF),
                sunken: pick(0x1B1B1F, 0xFFFFFF),
                line: pick(0x36363E, 0xE2E2E7),
                edge: pick(0x9A9AA3, 0x6B6B70),
                accent: pick(0x6EA0FF, 0x2F6FEB),
                on_accent: pick(0x101014, 0xFFFFFF),
                ok: pick(0x4CC38A, 0x1A7F4B),
                bad: pick(0xFF7B72, 0xC2362B),
                running,
                attention: running,
                ansi: ansi.map(hex),
            },
            text: Type {
                body: 14.0,
                answer: 15.0,
                secondary: 13.0,
                title: 15.0,
                request_title: 14.0,
                heading_large: 17.0,
                heading: 15.0,
                mono: 13.0,
                code: 12.0,
                meta: 12.0,
                label: 11.0,
                label_weight: FontWeight::SEMIBOLD,
                line_height: 1.5,
                family: None,
                mono_family: default_mono_family(),
            },
            badge: BadgeStyle::Outlined,
            card_radius: 10.0,
            control_radius: 6.0,
            card_padding: (10.0, 10.0),
            row_spacing: 10.0,
            waiting_border: 1.0,
            waiting_fill: 0.0,
            control_height: 28.0,
            folder_labels: false,
            dark,
        }
    }

    #[must_use]
    pub fn tone(&self, tone: Tone) -> Hsla {
        match tone {
            Tone::Ok => self.colors.ok,
            Tone::Bad => self.colors.bad,
            Tone::Running => self.colors.running,
            Tone::Attention => self.colors.attention,
        }
    }
}

/// The app's theme. The standard dark one until [`set_theme`] or [`crate::init`] is called.
#[must_use]
pub fn theme(cx: &App) -> Theme {
    cx.try_global::<Theme>()
        .cloned()
        .unwrap_or_else(|| Theme::standard(true))
}

/// Sets the theme every part is drawn with, and draws the windows again.
pub fn set_theme(cx: &mut App, theme: Theme) {
    cx.set_global(theme);
    cx.refresh_windows();
}

/// What a badge says about a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Bad,
    Running,
    Attention,
}

impl Tone {
    /// The tone a word of status has: done, failed, waiting for the user, or anything else running.
    #[must_use]
    pub fn of(status: &str) -> Self {
        match status {
            "completed" | "idle" | "exit 0" => Self::Ok,
            "failed" | "cancelled" => Self::Bad,
            "requires_action" | "needs you" => Self::Attention,
            _ if status.starts_with("exit") => Self::Bad,
            _ => Self::Running,
        }
    }
}
