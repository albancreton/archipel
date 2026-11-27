//! The agent's text. It is Markdown, and is built as GPUI's own elements: never as HTML, and
//! markup the agent writes is shown as text. Links go only to `http`, `https` and `mailto`;
//! images are named, not fetched. Text that is still arriving draws: an open code fence is code
//! up to where the text ends.

use crate::theme::{Theme, theme};
use gpui::{
    App, Div, Font, FontStyle, FontWeight, Hsla, InteractiveText, SharedString, StrikethroughStyle,
    StyledText, TextRun, UnderlineStyle, Window, div, prelude::*, px,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// How a stretch of text inside a paragraph is set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Span {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// Where it leads, when it is a link with a safe address.
    pub link: Option<String>,
}

/// Text with the style of each of its stretches.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<(Range<usize>, Span)>,
}

impl Inline {
    fn push(&mut self, text: &str, span: &Span) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        match self.spans.last_mut() {
            Some((range, last)) if last == span && range.end == start => {
                range.end = self.text.len()
            }
            _ => self.spans.push((start..self.text.len(), span.clone())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Inline),
    Heading(u8, Inline),
    Code(String),
    Quote(Vec<Block>),
    /// The number the list starts at when it is ordered, and each item's blocks.
    List(Option<u64>, Vec<Vec<Block>>),
    Rule,
    /// The heading row, then the others.
    Table(Vec<Inline>, Vec<Vec<Inline>>),
}

/// An address the kit lets a link open.
#[must_use]
pub fn safe_link(address: &str) -> bool {
    let lower = address.trim_start().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

enum Open {
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Vec<Block>>),
    Item(Vec<Block>),
    Table(Vec<Inline>, Vec<Vec<Inline>>, Vec<Inline>),
}

struct Builder {
    /// The finished blocks of the document.
    root: Vec<Block>,
    open: Vec<Open>,
    /// The text being collected, and what it becomes: a heading's level, or a paragraph.
    inline: Option<(Option<u8>, Inline)>,
    span: Span,
    /// The spans to return to when a style ends.
    spans: Vec<Span>,
    code: Option<String>,
}

impl Builder {
    fn blocks(&mut self) -> &mut Vec<Block> {
        match self.open.last_mut() {
            Some(Open::Quote(blocks) | Open::Item(blocks)) => blocks,
            // Text straight in a list or a table has no place of its own; it goes to the root.
            _ => &mut self.root,
        }
    }

    fn flush(&mut self) {
        if let Some((level, inline)) = self.inline.take() {
            if inline.text.trim().is_empty() {
                return;
            }
            let block = match level {
                Some(level) => Block::Heading(level, inline),
                None => Block::Paragraph(inline),
            };
            self.blocks().push(block);
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(code) = &mut self.code {
            code.push_str(text);
            return;
        }
        let span = self.span.clone();
        self.inline
            .get_or_insert_with(|| (None, Inline::default()))
            .1
            .push(text, &span);
    }

    fn style(&mut self, change: impl FnOnce(&mut Span)) {
        self.spans.push(self.span.clone());
        change(&mut self.span);
    }

    fn unstyle(&mut self) {
        if let Some(span) = self.spans.pop() {
            self.span = span;
        }
    }
}

/// Reads Markdown into blocks. Incomplete text is read as far as it goes.
#[must_use]
pub fn parse(source: &str) -> Vec<Block> {
    let mut builder = Builder {
        root: Vec::new(),
        open: Vec::new(),
        inline: None,
        span: Span::default(),
        spans: Vec::new(),
        code: None,
    };
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    builder.flush();
                    builder.inline = Some((None, Inline::default()));
                }
                Tag::Heading { level, .. } => {
                    builder.flush();
                    builder.inline = Some((Some(level as u8), Inline::default()));
                }
                Tag::CodeBlock(_) => {
                    builder.flush();
                    builder.code = Some(String::new());
                }
                Tag::BlockQuote(_) => {
                    builder.flush();
                    builder.open.push(Open::Quote(Vec::new()));
                }
                Tag::List(start) => {
                    builder.flush();
                    builder.open.push(Open::List(start, Vec::new()));
                }
                Tag::Item => {
                    builder.flush();
                    builder.open.push(Open::Item(Vec::new()));
                }
                Tag::Table(_) => {
                    builder.flush();
                    builder
                        .open
                        .push(Open::Table(Vec::new(), Vec::new(), Vec::new()));
                }
                Tag::TableCell => builder.inline = Some((None, Inline::default())),
                Tag::Emphasis => builder.style(|span| span.italic = true),
                Tag::Strong => builder.style(|span| span.bold = true),
                Tag::Strikethrough => builder.style(|span| span.strike = true),
                Tag::Link { dest_url, .. } => {
                    let link = safe_link(&dest_url).then(|| dest_url.to_string());
                    builder.style(|span| span.link = link);
                }
                // Named, not fetched: fetching tells the picture's server someone is reading.
                Tag::Image { .. } => builder.text("[image: "),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) => builder.flush(),
                TagEnd::CodeBlock => {
                    if let Some(code) = builder.code.take() {
                        let code = code.strip_suffix('\n').unwrap_or(&code).to_owned();
                        builder.blocks().push(Block::Code(code));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    builder.flush();
                    if let Some(Open::Quote(blocks)) = builder.open.pop() {
                        builder.blocks().push(Block::Quote(blocks));
                    }
                }
                TagEnd::Item => {
                    builder.flush();
                    if let Some(Open::Item(blocks)) = builder.open.pop()
                        && let Some(Open::List(_, items)) = builder.open.last_mut()
                    {
                        items.push(blocks);
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Open::List(start, items)) = builder.open.pop() {
                        builder.blocks().push(Block::List(start, items));
                    }
                }
                TagEnd::TableCell => {
                    let cell = builder
                        .inline
                        .take()
                        .map(|(_, inline)| inline)
                        .unwrap_or_default();
                    if let Some(Open::Table(_, _, row)) = builder.open.last_mut() {
                        row.push(cell);
                    }
                }
                TagEnd::TableHead => {
                    if let Some(Open::Table(head, _, row)) = builder.open.last_mut() {
                        *head = std::mem::take(row);
                    }
                }
                TagEnd::TableRow => {
                    if let Some(Open::Table(_, rows, row)) = builder.open.last_mut() {
                        rows.push(std::mem::take(row));
                    }
                }
                TagEnd::Table => {
                    if let Some(Open::Table(head, rows, _)) = builder.open.pop() {
                        builder.blocks().push(Block::Table(head, rows));
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    builder.unstyle();
                }
                TagEnd::Image => builder.text("]"),
                _ => {}
            },
            // Markup the agent writes is text, like the rest.
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => builder.text(&text),
            Event::Code(text) => {
                builder.style(|span| span.code = true);
                builder.text(&text);
                builder.unstyle();
            }
            Event::SoftBreak => builder.text(" "),
            Event::HardBreak => builder.text("\n"),
            Event::Rule => {
                builder.flush();
                builder.blocks().push(Block::Rule);
            }
            _ => {}
        }
    }
    builder.flush();
    // What is still open when the text ends is closed as it stands.
    while let Some(open) = builder.open.pop() {
        match open {
            Open::Quote(blocks) => builder.blocks().push(Block::Quote(blocks)),
            Open::Item(blocks) => {
                if let Some(Open::List(_, items)) = builder.open.last_mut() {
                    items.push(blocks);
                }
            }
            Open::List(start, items) => builder.blocks().push(Block::List(start, items)),
            Open::Table(head, rows, _) => builder.blocks().push(Block::Table(head, rows)),
        }
    }
    builder.root
}

struct Drawing<'a> {
    theme: &'a Theme,
    font: Font,
    color: Hsla,
    key: &'a str,
    /// Counts the texts with links, to give each its own id.
    links: usize,
    /// How wide the text's own prose gets; code and tables are not held to it.
    column: Option<f32>,
}

impl Drawing<'_> {
    fn inline(&mut self, inline: &Inline, weight: Option<FontWeight>) -> gpui::AnyElement {
        let theme = self.theme;
        let mut runs = Vec::new();
        let mut links: Vec<(Range<usize>, String)> = Vec::new();
        for (range, span) in &inline.spans {
            let mut font = self.font.clone();
            if span.code {
                font.family = theme.text.mono_family.clone();
            }
            font.weight = if span.bold {
                FontWeight::BOLD
            } else {
                weight.unwrap_or(font.weight)
            };
            if span.italic {
                font.style = FontStyle::Italic;
            }
            let color = if span.link.is_some() {
                theme.colors.accent
            } else {
                self.color
            };
            runs.push(TextRun {
                len: range.len(),
                font,
                color,
                background_color: span.code.then_some(theme.colors.raised),
                underline: span.link.as_ref().map(|_| UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(color),
                    wavy: false,
                }),
                strikethrough: span.strike.then(|| StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(color),
                }),
            });
            if let Some(link) = &span.link {
                links.push((range.clone(), link.clone()));
            }
        }
        let text = StyledText::new(SharedString::from(inline.text.clone())).with_runs(runs);
        if links.is_empty() {
            return text.into_any_element();
        }
        self.links += 1;
        let (ranges, addresses): (Vec<_>, Vec<_>) = links.into_iter().unzip();
        InteractiveText::new(
            SharedString::from(format!("{}/link{}", self.key, self.links)),
            text,
        )
        .on_click(ranges, move |index, _, cx| cx.open_url(&addresses[index]))
        .into_any_element()
    }

    fn blocks(&mut self, blocks: &[Block]) -> Div {
        let theme = self.theme;
        // Only the text's own blocks keep to the column, not those inside a list or a quote,
        // which are already in one.
        let limit = self.column.take();
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(theme.text.body * 0.6))
            .w_full()
            .min_w_0();
        for block in blocks {
            let wide = matches!(block, Block::Code(_) | Block::Table(..));
            let drawn = match block {
                Block::Paragraph(inline) => div().w_full().child(self.inline(inline, None)),
                // Kept close to the text's size: this is a chat, not a document.
                Block::Heading(level, inline) => div()
                    .w_full()
                    .text_size(px(if *level == 1 {
                        theme.text.heading_large
                    } else {
                        theme.text.heading
                    }))
                    .child(self.inline(inline, Some(FontWeight::SEMIBOLD))),
                Block::Code(code) => code_block_themed(code, theme),
                Block::Quote(inner) => div()
                    .w_full()
                    .pl(px(12.0))
                    .border_l_2()
                    .border_color(theme.colors.edge)
                    .child(self.blocks(inner)),
                Block::List(start, items) => {
                    let mut list = div().flex().flex_col().gap(px(4.0)).w_full();
                    for (index, item) in items.iter().enumerate() {
                        let mark = start.map_or_else(
                            || "•".to_owned(),
                            |start| format!("{}.", start + index as u64),
                        );
                        list = list.child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(8.0))
                                .w_full()
                                .child(
                                    div()
                                        .flex_none()
                                        .min_w(px(theme.text.body))
                                        .text_color(theme.colors.secondary)
                                        .child(SharedString::from(mark)),
                                )
                                .child(div().flex_1().min_w_0().child(self.blocks(item))),
                        );
                    }
                    list
                }
                Block::Rule => div().w_full().h(px(1.0)).bg(theme.colors.line),
                Block::Table(head, rows) => {
                    let mut table = div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .rounded(px(theme.control_radius))
                        .border_1()
                        .border_color(theme.colors.line)
                        .overflow_hidden();
                    let all =
                        std::iter::once((true, head)).chain(rows.iter().map(|row| (false, row)));
                    for (index, (is_head, cells)) in all.enumerate() {
                        let mut row = div().flex().w_full();
                        if index > 0 {
                            row = row.border_t_1().border_color(theme.colors.line);
                        }
                        if is_head {
                            row = row.bg(theme.colors.card);
                        }
                        for cell in cells {
                            let weight = is_head.then_some(FontWeight::SEMIBOLD);
                            row = row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px(px(8.0))
                                    .py(px(4.0))
                                    .child(self.inline(cell, weight)),
                            );
                        }
                        table = table.child(row);
                    }
                    table
                }
            };
            column = column.child(match limit {
                Some(width) if !wide => in_column(drawn, width),
                _ => drawn,
            });
        }
        column
    }
}

/// A block in a column of its own width, limited to the row's. Not a limit in pixels on the
/// block: GPUI then measures its text as one line.
fn in_column(block: Div, width: f32) -> Div {
    div().flex().w_full().child(
        div()
            .w(px(width))
            .max_w(gpui::relative(1.0))
            .min_w_0()
            .child(block),
    )
}

fn code_block_themed(code: &str, theme: &Theme) -> Div {
    div()
        .w_full()
        .p(px(8.0))
        .rounded(px(theme.control_radius))
        .bg(theme.colors.sunken)
        .border_1()
        .border_color(theme.colors.line)
        .font_family(theme.text.mono_family.clone())
        .text_size(px(theme.text.code))
        .child(SharedString::from(code.to_owned()))
}

/// The agent's text, drawn. `key` names it among the other texts of the view (links need an id
/// of their own); `color` is the text's colour, the secondary one for reasoning.
pub fn markdown(key: &str, source: &str, color: Hsla, window: &Window, cx: &App) -> Div {
    draw(key, source, color, None, window, cx)
}

/// The same, with its prose (paragraphs, headings, lists, quotes) in a column `column` wide
/// at most. Code blocks and tables take the whole width they are given.
pub fn markdown_in_column(
    key: &str,
    source: &str,
    color: Hsla,
    column: f32,
    window: &Window,
    cx: &App,
) -> Div {
    draw(key, source, color, Some(column), window, cx)
}

fn draw(
    key: &str,
    source: &str,
    color: Hsla,
    column: Option<f32>,
    window: &Window,
    cx: &App,
) -> Div {
    let theme = theme(cx);
    let mut font = window.text_style().font();
    if let Some(family) = &theme.text.family {
        font.family = family.clone();
    }
    let mut drawing = Drawing {
        theme: &theme,
        font,
        color,
        key,
        links: 0,
        column,
    };
    drawing.blocks(&parse(source)).text_color(color)
}

/// Text as it was written, with its line breaks: the user's own, never read as Markdown.
pub fn plain(text: &str) -> Div {
    div().w_full().child(SharedString::from(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(block: &Block) -> &Inline {
        match block {
            Block::Paragraph(inline) => inline,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    #[test]
    fn styles_and_links_are_read() {
        let blocks = parse(
            "Some **bold** and `code` and [a link](https://example.com) and [bad](javascript:alert(1)).",
        );
        let inline = paragraph(&blocks[0]);
        assert_eq!(inline.text, "Some bold and code and a link and bad.");
        let styled: Vec<_> = inline
            .spans
            .iter()
            .map(|(range, span)| (&inline.text[range.clone()], span))
            .collect();
        assert!(
            styled
                .iter()
                .any(|(text, span)| *text == "bold" && span.bold)
        );
        assert!(
            styled
                .iter()
                .any(|(text, span)| *text == "code" && span.code)
        );
        assert!(
            styled.iter().any(|(text, span)| *text == "a link"
                && span.link.as_deref() == Some("https://example.com"))
        );
        assert!(
            styled
                .iter()
                .all(|(text, span)| !text.contains("bad") || span.link.is_none())
        );
    }

    #[test]
    fn markup_is_text_and_images_are_named() {
        let blocks = parse(
            "<script>alert(1)</script>\n\nInline <b>bold</b> and ![a cat](https://example.com/cat.png)",
        );
        let text: String = blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph(inline) => inline.text.clone(),
                _ => String::new(),
            })
            .collect();
        assert!(text.contains("<script>alert(1)</script>"));
        assert!(text.contains("Inline <b>bold</b> and [image: a cat]"));
    }

    #[test]
    fn an_open_fence_is_code_to_the_end() {
        let blocks = parse("Here:\n\n```rust\nfn main() {");
        assert_eq!(blocks[1], Block::Code("fn main() {".to_owned()));
    }

    #[test]
    fn lists_quotes_and_tables_nest() {
        let blocks =
            parse("1. one\n2. two\n   - inner\n\n> quoted\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        let Block::List(Some(1), items) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[1].last(), Some(Block::List(None, _))));
        assert!(matches!(&blocks[1], Block::Quote(inner) if paragraph(&inner[0]).text == "quoted"));
        let Block::Table(head, rows) = &blocks[2] else {
            panic!("{blocks:?}")
        };
        assert_eq!(
            (head.len(), rows.len(), rows[0][1].text.as_str()),
            (2, 1, "2")
        );
    }
}
