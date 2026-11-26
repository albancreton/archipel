//! A text field. GPUI has none of its own, so the kit draws one: a caret, a selection, the
//! clipboard, the system's input methods, and more than one line when asked.
//!
//! It follows the text input example that ships with GPUI, with wrapping and line breaks added.
//! [`bind_keys`] (called by [`crate::init`]) gives it its keys.

use crate::theme::theme;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, SharedString, Style, TextAlign, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px,
    relative, size,
};
use std::ops::Range;

actions!(
    archipel_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Submit,
        Newline,
        Complete,
        Cancel,
    ]
);

/// The key context of a field, for an app's own bindings.
pub const KEY_CONTEXT: &str = "ArchipelField";

/// Binds the keys a field answers to. `secondary` is Command on macOS and Control elsewhere.
pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("secondary-v", Paste, context),
        KeyBinding::new("secondary-c", Copy, context),
        KeyBinding::new("secondary-x", Cut, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("enter", Submit, context),
        KeyBinding::new("shift-enter", Newline, context),
        KeyBinding::new("tab", Complete, context),
        KeyBinding::new("escape", Cancel, context),
    ]);
}

/// What a field tells whoever subscribed to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldEvent {
    /// The text changed.
    Changed,
    /// Enter was pressed.
    Submitted,
    /// Escape was pressed.
    Cancelled,
}

pub struct TextField {
    focus: FocusHandle,
    content: String,
    placeholder: SharedString,
    /// More than one line: Shift+Enter breaks the line, and long text wraps.
    multiline: bool,
    disabled: bool,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    lines: Vec<WrappedLine>,
    bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    selecting: bool,
}

impl EventEmitter<FieldEvent> for TextField {}

impl TextField {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            content: String::new(),
            placeholder: placeholder.into(),
            multiline: false,
            disabled: false,
            selected: 0..0,
            reversed: false,
            marked: None,
            lines: Vec::new(),
            bounds: None,
            line_height: px(20.0),
            selecting: false,
        }
    }

    #[must_use]
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.content = text.into();
        self.selected = self.content.len()..self.content.len();
        self
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replaces the text and puts the caret at its end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.content = text.into();
        self.selected = self.content.len()..self.content.len();
        self.reversed = false;
        self.marked = None;
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.into();
        if self.placeholder != placeholder {
            self.placeholder = placeholder;
            cx.notify();
        }
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    /// Where the caret is, as a byte offset in the text.
    #[must_use]
    pub fn caret(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.previous_boundary(self.caret()), cx);
        } else {
            self.move_to(self.selected.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.next_boundary(self.caret()), cx);
        } else {
            self.move_to(self.selected.end, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_rows(-1.0, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_rows(1.0, cx);
    }

    /// Moves the caret a row up or down, keeping its place across.
    fn move_rows(&mut self, by: f32, cx: &mut Context<Self>) {
        let Some(at) = self.position_of(self.caret()) else {
            return;
        };
        let wanted = at.y + self.line_height * by + self.line_height / 2.0;
        let to = if wanted < px(0.0) {
            0
        } else {
            self.index_at(point(at.x, wanted))
        };
        self.move_to(to, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.caret()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.caret()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.content[..self.caret()]
            .rfind('\n')
            .map_or(0, |at| at + 1);
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let caret = self.caret();
        let end = self.content[caret..]
            .find('\n')
            .map_or(self.content.len(), |at| caret + at);
        self.move_to(end, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.previous_boundary(self.caret()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.next_boundary(self.caret()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::Submitted);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            cx.emit(FieldEvent::Submitted);
        }
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::Cancelled);
        cx.propagate();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline {
                text.replace("\r\n", "\n")
            } else {
                text.replace(['\n', '\r'], " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_owned(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_owned(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus);
        self.selecting = true;
        let at = self.index_for_mouse(event.position);
        if event.modifiers.shift {
            self.select_to(at, cx);
        } else {
            self.move_to(at, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_for_mouse(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected = offset..offset;
        self.reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        cx.notify();
    }

    /// Where each shaped line starts in the text, with the line.
    fn line_starts(&self) -> impl Iterator<Item = (usize, &WrappedLine)> {
        let mut start = 0;
        self.lines.iter().map(move |line| {
            let at = start;
            start += line.len() + 1;
            (at, line)
        })
    }

    /// The top left of the caret at `index`, from the field's own top left.
    fn position_of(&self, index: usize) -> Option<Point<Pixels>> {
        let mut top = px(0.0);
        for (start, line) in self.line_starts() {
            if index <= start + line.len() {
                let inside =
                    line.position_for_index(index.saturating_sub(start), self.line_height)?;
                return Some(point(inside.x, top + inside.y));
            }
            top += line.size(self.line_height).height;
        }
        None
    }

    /// The offset in the text nearest to a point given from the field's own top left.
    fn index_at(&self, position: Point<Pixels>) -> usize {
        let mut top = px(0.0);
        for (start, line) in self.line_starts() {
            let height = line.size(self.line_height).height;
            if position.y < top + height {
                let inside = point(position.x, (position.y - top).max(px(0.0)));
                let (Ok(at) | Err(at)) = line.closest_index_for_position(inside, self.line_height);
                return start + at;
            }
            top += height;
        }
        self.content.len()
    }

    fn index_for_mouse(&self, position: Point<Pixels>) -> usize {
        match self.bounds {
            Some(bounds) if !self.content.is_empty() => self.index_at(position - bounds.origin),
            _ => 0,
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let (mut utf8, mut utf16) = (0, 0);
        for character in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += character.len_utf16();
            utf8 += character.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let (mut utf8, mut utf16) = (0, 0);
        for character in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += character.len_utf8();
            utf16 += character.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content[offset..]
            .chars()
            .next()
            .map_or(self.content.len(), |character| {
                offset + character.len_utf8()
            })
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        let new_text = if self.multiline {
            new_text.to_owned()
        } else {
            new_text.replace(['\n', '\r'], " ")
        };
        self.content.replace_range(range.clone(), &new_text);
        let caret = range.start + new_text.len();
        self.selected = caret..caret;
        self.reversed = false;
        self.marked = None;
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.content.replace_range(range.clone(), new_text);
        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected = new_selected_range_utf16
            .as_ref()
            .map(|new_range| self.range_from_utf16(new_range))
            .map_or_else(
                || range.start + new_text.len()..range.start + new_text.len(),
                |new_range| new_range.start + range.start..new_range.end + range.start,
            );
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let start = self.position_of(range.start)?;
        let end = self.position_of(range.end).unwrap_or(start);
        Some(Bounds::from_corners(
            bounds.origin + start,
            bounds.origin + point(end.x.max(start.x), start.y + self.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.bounds?;
        Some(self.offset_to_utf16(self.index_at(position - bounds.origin)))
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        let focused = self.focus.is_focused(window);
        div()
            .id("field")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::cancel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .min_h(px(theme.control_height))
            .px(px(10.0))
            .py(px((theme.control_height
                - theme.text.body * theme.text.line_height)
                .max(0.0)
                / 2.0))
            .rounded(px(theme.control_radius))
            .border_1()
            .border_color(if focused {
                theme.colors.accent
            } else {
                theme.colors.line
            })
            .bg(theme.colors.sunken)
            .text_color(theme.colors.text)
            .text_size(px(theme.text.body))
            .line_height(relative(theme.text.line_height))
            .when(self.disabled, |field| field.opacity(0.6))
            .child(FieldElement { field: cx.entity() })
    }
}

/// The text of a field, its selection and its caret.
struct FieldElement {
    field: Entity<TextField>,
}

struct Shaped {
    lines: Vec<WrappedLine>,
    selection: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
}

impl FieldElement {
    /// The text to shape (the placeholder when there is none) and its runs.
    fn runs(&self, window: &Window, cx: &App) -> (SharedString, Vec<TextRun>) {
        let field = self.field.read(cx);
        let style = window.text_style();
        if field.content.is_empty() {
            let run = TextRun {
                len: field.placeholder.len(),
                font: style.font(),
                color: theme(cx).colors.secondary,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            return (field.placeholder.clone(), vec![run]);
        }
        let run = TextRun {
            len: field.content.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match field.marked.clone() {
            // What an input method is still composing is underlined.
            Some(marked) => vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: field.content.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        (SharedString::from(field.content.clone()), runs)
    }
}

impl IntoElement for FieldElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FieldElement {
    type RequestLayoutState = ();
    type PrepaintState = Shaped;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (text, runs) = self.runs(window, cx);
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        // As tall as the text is once wrapped to the width the field gets.
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let rows: usize = window
                .text_system()
                .shape_text(text.clone(), font_size, &runs, width, None)
                .map(|lines| {
                    lines
                        .iter()
                        .map(|line| line.wrap_boundaries().len() + 1)
                        .sum()
                })
                .unwrap_or(1);
            size(width.unwrap_or(px(200.0)), line_height * rows.max(1))
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (text, runs) = self.runs(window, cx);
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let lines: Vec<WrappedLine> = window
            .text_system()
            .shape_text(text, font_size, &runs, Some(bounds.size.width), None)
            .map(|lines| lines.into_iter().collect())
            .unwrap_or_default();

        let (selected, caret, empty) = self.field.update(cx, |field, _| {
            field.line_height = line_height;
            field.bounds = Some(bounds);
            // Positions are asked of the field, so it needs the lines of this frame now.
            field.lines = if field.content.is_empty() {
                Vec::new()
            } else {
                lines.clone()
            };
            (
                field.selected.clone(),
                field.caret(),
                field.content.is_empty(),
            )
        });
        let field = self.field.read(cx);
        let at = |index: usize| {
            if empty {
                Some(point(px(0.0), px(0.0)))
            } else {
                field.position_of(index)
            }
        };
        let mut selection = Vec::new();
        if !selected.is_empty()
            && let (Some(start), Some(end)) = (at(selected.start), at(selected.end))
        {
            let row = |from: Point<Pixels>, to_x: Pixels| {
                Bounds::new(
                    bounds.origin + from,
                    size((to_x - from.x).max(px(2.0)), line_height),
                )
            };
            if start.y == end.y {
                selection.push(row(start, end.x));
            } else {
                // The first row to its end, whole rows between, and the last row from its start.
                selection.push(row(start, bounds.size.width));
                let mut y = start.y + line_height;
                while y < end.y {
                    selection.push(row(point(px(0.0), y), bounds.size.width));
                    y += line_height;
                }
                selection.push(row(point(px(0.0), end.y), end.x));
            }
        }
        let caret = at(caret)
            .filter(|_| selected.is_empty())
            .map(|at| Bounds::new(bounds.origin + at, size(px(1.5), line_height)));
        Shaped {
            lines,
            selection,
            caret,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        shaped: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let theme = theme(cx);
        let focus = self.field.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );
        for selection in &shaped.selection {
            window.paint_quad(fill(*selection, theme.colors.accent.opacity(0.35)));
        }
        let line_height = window.line_height();
        let mut origin = bounds.origin;
        for line in &shaped.lines {
            let _ = line.paint(
                origin,
                line_height,
                TextAlign::Left,
                Some(bounds),
                window,
                cx,
            );
            origin.y += line.size(line_height).height;
        }
        if focus.is_focused(window)
            && let Some(caret) = shaped.caret
        {
            window.paint_quad(fill(caret, theme.colors.accent));
        }
    }
}
