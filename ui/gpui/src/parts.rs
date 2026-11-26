//! The small pieces most components use: badge, label, card, fold, code block, chip, meter,
//! notice, problem, button, menu. They are most of a set's look.

use crate::ansi;
use crate::memory;
use crate::theme::{BadgeStyle, Theme, Tone, mix, theme};
use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Div, FontWeight, Hsla, IntoElement,
    PathBuilder, Render, SharedString, Stateful, StyledText, Window, anchored, canvas, deferred,
    div, point, prelude::*, px, relative,
};
use std::rc::Rc;

/// Something the user can press: the window and the app, as GPUI hands them to a listener.
pub type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Draws something when asked: a part that is built again at each drawing.
pub type Draw = Box<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// Called with the value the user picked in a menu.
pub type Pick = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// A small drawn mark. Each is a few strokes on a 16 by 16 grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    ChevronRight,
    ChevronDown,
    Check,
    Close,
    Pencil,
    Trash,
}

impl Icon {
    fn strokes(self) -> &'static [&'static [(f32, f32)]] {
        match self {
            Self::ChevronRight => &[&[(6.0, 3.5), (10.5, 8.0), (6.0, 12.5)]],
            Self::ChevronDown => &[&[(3.5, 6.0), (8.0, 10.5), (12.5, 6.0)]],
            Self::Check => &[&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)]],
            Self::Close => &[&[(4.0, 4.0), (12.0, 12.0)], &[(12.0, 4.0), (4.0, 12.0)]],
            Self::Pencil => &[
                &[
                    (3.0, 13.0),
                    (3.6, 10.4),
                    (10.5, 3.5),
                    (12.5, 5.5),
                    (5.6, 12.4),
                    (3.0, 13.0),
                ],
                &[(9.0, 5.0), (11.0, 7.0)],
            ],
            Self::Trash => &[
                &[(3.0, 4.5), (13.0, 4.5)],
                &[(6.0, 4.5), (6.0, 3.0), (10.0, 3.0), (10.0, 4.5)],
                &[(4.5, 4.5), (5.2, 13.0), (10.8, 13.0), (11.5, 4.5)],
                &[(7.0, 7.0), (7.0, 10.5)],
                &[(9.0, 7.0), (9.0, 10.5)],
            ],
        }
    }
}

/// An icon, `size` pixels square, stroked in `color`.
pub fn icon(icon: Icon, size: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let scale = size / 16.0;
            for stroke in icon.strokes() {
                let mut path = PathBuilder::stroke(px(1.6 * scale.max(0.8)));
                for (index, (x, y)) in stroke.iter().enumerate() {
                    let at = bounds.origin + point(px(x * scale), px(y * scale));
                    if index == 0 {
                        path.move_to(at);
                    } else {
                        path.line_to(at);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .size(px(size))
    .flex_none()
}

/// A short word in a pill, coloured by tone. Without a tone it follows the word. `_` is shown
/// as a space.
pub fn badge(text: &str, tone: Option<Tone>, cx: &App) -> Div {
    let theme = theme(cx);
    let color = theme.tone(tone.unwrap_or_else(|| Tone::of(text)));
    let words = SharedString::from(text.replace('_', " "));
    let pill = div()
        .flex()
        .flex_none()
        .items_center()
        .rounded_full()
        .text_size(px(theme.text.meta))
        .line_height(relative(1.4))
        .whitespace_nowrap();
    match theme.badge {
        BadgeStyle::Outlined => pill
            .px(px(7.0))
            .border_1()
            .border_color(color)
            .text_color(color)
            .child(words),
        BadgeStyle::Tinted => pill
            .gap(px(6.0))
            .px(px(10.0))
            .py(px(1.0))
            .bg(color.opacity(0.12))
            // Text between the tone and the text colour, so it stays readable on the pale fill.
            .text_color(mix(color, theme.colors.text, 0.7))
            .child(div().size(px(7.0)).rounded_full().bg(color))
            .child(words),
    }
}

/// The uppercase word that names a part.
pub fn label(text: &str, cx: &App) -> Div {
    let theme = theme(cx);
    div()
        .flex_none()
        .text_size(px(theme.text.label))
        .font_weight(theme.text.label_weight)
        .text_color(theme.colors.secondary)
        .child(SharedString::from(text.to_uppercase()))
}

/// A bordered, softly filled box. `highlighted` while it waits for the user.
pub fn card(highlighted: bool, cx: &App) -> Div {
    let theme = theme(cx);
    let (across, down) = theme.card_padding;
    let fill = if highlighted {
        theme
            .colors
            .card
            .blend(theme.colors.attention.opacity(theme.waiting_fill))
    } else {
        theme.colors.card
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .w_full()
        .px(px(across))
        .py(px(down))
        .rounded(px(theme.card_radius))
        .bg(fill)
        .border(px(if highlighted {
            theme.waiting_border
        } else {
            1.0
        }))
        .border_color(if highlighted {
            theme.colors.attention
        } else {
            theme.colors.line
        })
}

/// A one-line summary that opens to show more. `key` is where the kit remembers whether it is
/// open; `open` is how it starts.
pub fn fold(
    key: &str,
    title: impl IntoElement,
    open: bool,
    cx: &App,
    body: impl FnOnce() -> AnyElement,
) -> Div {
    let theme = theme(cx);
    let is_open = memory::flag(cx, key, open);
    let key: SharedString = key.to_owned().into();
    let name = key.to_string();
    let summary = div()
        .id(key.clone())
        .debug_selector(move || name)
        .flex()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .text_size(px(theme.text.secondary))
        .text_color(theme.colors.secondary)
        .hover(|summary| summary.text_color(theme.colors.text))
        .child(icon(
            if is_open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            12.0,
            theme.colors.secondary,
        ))
        .child(div().flex_1().min_w_0().truncate().child(title))
        .on_click(move |_, _, cx| memory::set_flag(cx, &key, !is_open));
    div()
        .flex()
        .flex_col()
        .w_full()
        .child(summary)
        .when(is_open, |fold| {
            fold.child(div().pt(px(6.0)).pl(px(18.0)).child(body()))
        })
}

fn code_box(theme: &Theme) -> Div {
    div()
        .w_full()
        .p(px(8.0))
        .rounded(px(theme.control_radius))
        .bg(theme.colors.sunken)
        .border_1()
        .border_color(theme.colors.line)
        .font_family(theme.text.mono_family.clone())
        .text_size(px(theme.text.code))
        .line_height(relative(1.45))
        .text_color(theme.colors.text)
}

/// Monospaced text in a bordered box, wrapping long lines.
pub fn code_block(text: &str, cx: &App) -> Div {
    code_box(&theme(cx)).child(SharedString::from(text.to_owned()))
}

/// Terminal output in a code block, with the colours and styles the command printed.
pub fn terminal_block(output: &str, cx: &App) -> Div {
    let theme = theme(cx);
    code_box(&theme).child(ansi_text(output, &theme))
}

/// Terminal output with its colours: the 16 of the theme, the 256-colour table and exact ones.
pub fn ansi_text(output: &str, theme: &Theme) -> StyledText {
    let (text, highlights) = ansi::highlighted(output, theme);
    StyledText::new(text).with_highlights(highlights)
}

/// A small rounded name.
pub fn chip(text: &str, cx: &App) -> Div {
    let theme = theme(cx);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.0))
        .max_w(px(320.0))
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(theme.control_radius))
        .bg(theme.colors.raised)
        .text_size(px(theme.text.meta))
        .child(div().truncate().child(SharedString::from(text.to_owned())))
}

/// A thin bar, full to a percentage. The attention colour from 80%.
pub fn meter(percent: u32, cx: &App) -> Div {
    let theme = theme(cx);
    let width = 48.0;
    div()
        .flex_none()
        .w(px(width))
        .h(px(6.0))
        .rounded_full()
        .bg(theme.colors.line)
        .child(
            div()
                .h_full()
                .w(px(width * percent.min(100) as f32 / 100.0))
                .rounded_full()
                .bg(if percent >= 80 {
                    theme.colors.attention
                } else {
                    theme.colors.edge
                }),
        )
}

/// How a notice is meant: something is on its way, the user has something to do, or it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeTone {
    Wait,
    Act,
    Bad,
}

/// A line across the view with a tone and maybe one button.
pub fn notice(
    id: impl Into<SharedString>,
    text: &str,
    tone: NoticeTone,
    action: Option<(&str, Handler)>,
    cx: &App,
) -> Div {
    let theme = theme(cx);
    let color = match tone {
        NoticeTone::Bad => theme.colors.bad,
        NoticeTone::Act => theme.colors.attention,
        NoticeTone::Wait => theme.colors.secondary,
    };
    let id: SharedString = id.into();
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .w_full()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(theme.card_radius))
        .bg(color.opacity(0.1))
        .border_1()
        .border_color(color.opacity(0.6))
        .text_size(px(theme.text.secondary))
        .text_color(if tone == NoticeTone::Wait {
            theme.colors.secondary
        } else {
            theme.colors.text
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(SharedString::from(text.to_owned())),
        )
        .when_some(action, |notice, (title, run)| {
            notice
                .child(button(id, title, false, cx).on_click(move |_, window, cx| run(window, cx)))
        })
}

/// A line in the error colour, under the control that failed.
pub fn problem(text: &str, cx: &App) -> Div {
    let theme = theme(cx);
    div()
        .w_full()
        .text_size(px(theme.text.secondary))
        .text_color(theme.colors.bad)
        .child(SharedString::from(text.to_owned()))
}

/// A button. Add `.on_click`. `primary` is the one most often taken.
pub fn button(id: impl Into<SharedString>, title: &str, primary: bool, cx: &App) -> Stateful<Div> {
    let theme = theme(cx);
    let (fill, text, border) = if primary {
        (
            theme.colors.accent,
            theme.colors.on_accent,
            theme.colors.accent,
        )
    } else {
        (theme.colors.raised, theme.colors.text, theme.colors.line)
    };
    let edge = theme.colors.edge;
    let id: SharedString = id.into();
    let name = id.to_string();
    div()
        .id(id)
        .debug_selector(move || name)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(theme.control_height))
        .px(px(12.0))
        .rounded(px(theme.control_radius))
        .bg(fill)
        .border_1()
        .border_color(border)
        .text_color(text)
        .text_size(px(theme.text.secondary))
        .when(primary, |button| button.font_weight(FontWeight::MEDIUM))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |button| {
            if primary {
                button.opacity(0.9)
            } else {
                button.border_color(edge)
            }
        })
        .active(|button| button.opacity(0.75))
        .child(SharedString::from(title.to_owned()))
}

/// The same button, greyed and deaf.
pub fn disabled(button: Stateful<Div>) -> Stateful<Div> {
    button.opacity(0.5).cursor_default()
}

/// A button that is only an icon, named by its tooltip.
pub fn icon_button(id: impl Into<SharedString>, mark: Icon, name: &str, cx: &App) -> Stateful<Div> {
    let theme = theme(cx);
    let id: SharedString = id.into();
    let selector = id.to_string();
    div()
        .id(id)
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(theme.control_height))
        .rounded(px(theme.control_radius))
        .cursor_pointer()
        .hover(|button| button.bg(theme.colors.raised))
        .child(icon(mark, 16.0, theme.colors.secondary))
        .tooltip(tip(name))
}

/// Text that looks like a link and is pressed like a button.
pub fn link(id: impl Into<SharedString>, title: &str, cx: &App) -> Stateful<Div> {
    let theme = theme(cx);
    let id: SharedString = id.into();
    let name = id.to_string();
    div()
        .id(id)
        .debug_selector(move || name)
        .flex_none()
        .cursor_pointer()
        .text_color(theme.colors.accent)
        .text_size(px(theme.text.secondary))
        .underline()
        .child(SharedString::from(title.to_owned()))
}

struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        div()
            .max_w(px(360.0))
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(theme.control_radius))
            .bg(theme.colors.raised)
            .border_1()
            .border_color(theme.colors.edge)
            .text_color(theme.colors.text)
            .text_size(px(theme.text.meta))
            .child(self.0.clone())
    }
}

/// What is shown when the pointer rests on something: `.tooltip(tip("…"))`.
pub fn tip(text: &str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = SharedString::from(text.to_owned());
    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

/// One value a menu offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

/// Values under a heading. `name` is `None` for values without one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuGroup {
    pub name: Option<String>,
    pub choices: Vec<MenuChoice>,
}

/// A menu of values: a button with the current one, opening to the list. `key` names it in the
/// kit's memory. Without `pick` it is shown and cannot be opened.
pub fn menu(
    key: &str,
    title: Option<&str>,
    current: &str,
    groups: &[MenuGroup],
    pick: Option<Pick>,
    cx: &App,
) -> Div {
    let theme = theme(cx);
    let open = pick.is_some() && memory::open_menu(cx).as_deref() == Some(key);
    let shown = groups
        .iter()
        .flat_map(|group| &group.choices)
        .find(|choice| choice.value == current)
        .map_or(current, |choice| choice.name.as_str());
    let key_owned = key.to_owned();
    let toggle = div()
        .id(SharedString::from(format!("{key}/toggle")))
        .debug_selector(|| format!("{key}/toggle"))
        .flex()
        .items_center()
        .gap(px(6.0))
        .h(px(theme.control_height))
        .pl(px(10.0))
        .pr(px(8.0))
        .rounded(px(theme.control_radius))
        .bg(theme.colors.raised)
        .border_1()
        .border_color(if open {
            theme.colors.accent
        } else {
            theme.colors.line
        })
        .text_size(px(theme.text.secondary))
        .whitespace_nowrap()
        .when_some(title, |toggle, title| {
            toggle.child(
                div()
                    .text_color(theme.colors.secondary)
                    .child(SharedString::from(title.to_owned())),
            )
        })
        .child(SharedString::from(shown.to_owned()))
        .child(icon(Icon::ChevronDown, 12.0, theme.colors.secondary))
        .map(|toggle| match &pick {
            // Pressed while open, the list's own "pressed outside" closes it first: `open` is
            // what it was when this was drawn, so the two do not undo each other.
            Some(_) => toggle
                .cursor_pointer()
                .hover(|toggle| toggle.border_color(theme.colors.edge))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    memory::set_open_menu(cx, (!open).then(|| key_owned.clone()));
                }),
            None => toggle.opacity(0.6),
        });
    let list = pick.filter(|_| open).map(|pick| {
        let mut list = div()
            .id(SharedString::from(format!("{key}/list")))
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(180.0))
            .max_h(px(320.0))
            .overflow_y_scroll()
            .p(px(4.0))
            .rounded(px(theme.card_radius))
            .bg(theme.colors.raised)
            .border_1()
            .border_color(theme.colors.edge)
            .shadow_lg()
            .text_size(px(theme.text.secondary))
            .text_color(theme.colors.text)
            .on_mouse_down_out(|_, _, cx| memory::set_open_menu(cx, None));
        for (group_index, group) in groups.iter().enumerate() {
            if let Some(name) = &group.name {
                list = list.child(
                    div()
                        .px(px(8.0))
                        .pt(px(6.0))
                        .pb(px(2.0))
                        .child(label(name, cx)),
                );
            }
            for (index, choice) in group.choices.iter().enumerate() {
                let (pick, value) = (pick.clone(), choice.value.clone());
                let chosen = choice.value == current;
                list = list.child(
                    div()
                        .id(SharedString::from(format!("{key}/{group_index}/{index}")))
                        .debug_selector(|| format!("{key}/{}", choice.value))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(theme.control_radius))
                        .cursor_pointer()
                        .whitespace_nowrap()
                        .hover(|row| row.bg(theme.colors.accent.opacity(0.22)))
                        .child(div().w(px(14.0)).when(chosen, |mark| {
                            mark.child(icon(Icon::Check, 14.0, theme.colors.accent))
                        }))
                        .child(SharedString::from(choice.name.clone()))
                        .when_some(choice.description.as_deref(), |row, text| {
                            row.tooltip(tip(text))
                        })
                        .on_click(move |_, window, cx| {
                            memory::set_open_menu(cx, None);
                            pick(&value, window, cx);
                        }),
                );
            }
        }
        div()
            .absolute()
            .top_full()
            .left_0()
            .mt(px(4.0))
            .child(deferred(anchored().snap_to_window().child(list)).with_priority(1))
    });
    div().relative().flex_none().child(toggle).children(list)
}

/// A switch with its name: a box that is ticked or not.
pub fn checkbox(id: impl Into<SharedString>, name: &str, on: bool, cx: &App) -> Stateful<Div> {
    let theme = theme(cx);
    let id: SharedString = id.into();
    let selector = id.to_string();
    div()
        .id(id)
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .h(px(theme.control_height))
        .text_size(px(theme.text.secondary))
        .whitespace_nowrap()
        .cursor_pointer()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(18.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(if on {
                    theme.colors.accent
                } else {
                    theme.colors.edge
                })
                .bg(if on {
                    theme.colors.accent
                } else {
                    theme.colors.sunken
                })
                .when(on, |mark| {
                    mark.child(icon(Icon::Check, 14.0, theme.colors.on_accent))
                }),
        )
        .child(SharedString::from(name.to_owned()))
}

/// The mark before a plan entry or a todo: a tick when done, a dot while in progress, a ring before.
pub fn plan_mark(status: &str, cx: &App) -> Div {
    let theme = theme(cx);
    let mark = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.0));
    match status {
        "completed" => mark.child(icon(Icon::Check, 14.0, theme.colors.ok)),
        "in_progress" => mark.child(div().size(px(8.0)).rounded_full().bg(theme.colors.running)),
        _ => mark.child(
            div()
                .size(px(10.0))
                .rounded_full()
                .border(px(1.5))
                .border_color(theme.colors.secondary),
        ),
    }
}
