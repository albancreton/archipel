//! Terminal output with the colours and styles a command prints. The colour codes are read; the
//! other control codes are dropped. Not a terminal emulator: nothing moves the cursor except a
//! return to the start of the line, which is how progress bars redraw.

use crate::theme::Theme;
use gpui::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba, UnderlineStyle, px};
use std::ops::Range;

/// A colour in terminal output: one of the terminal's own 16, or an exact one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Palette(usize),
    Rgb(u8, u8, u8),
}

impl AnsiColor {
    /// A colour of the 256-colour table.
    #[must_use]
    pub fn indexed(n: usize) -> Self {
        let n = n.min(255);
        if n < 16 {
            return Self::Palette(n);
        }
        if n >= 232 {
            let grey = (8 + (n - 232) * 10) as u8;
            return Self::Rgb(grey, grey, grey);
        }
        let step = |value: usize| {
            if value == 0 {
                0
            } else {
                (55 + value * 40) as u8
            }
        };
        let at = n - 16;
        Self::Rgb(step(at / 36), step(at / 6 % 6), step(at % 6))
    }

    fn color(self, theme: &Theme) -> Hsla {
        match self {
            Self::Rgb(r, g, b) => Rgba {
                r: f32::from(r) / 255.0,
                g: f32::from(g) / 255.0,
                b: f32::from(b) / 255.0,
                a: 1.0,
            }
            .into(),
            Self::Palette(index) => theme
                .colors
                .ansi
                .get(index)
                .copied()
                .unwrap_or(theme.colors.text),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnsiStyle {
    pub fg: Option<AnsiColor>,
    pub bg: Option<AnsiColor>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl AnsiStyle {
    fn apply(&mut self, codes: &[usize]) {
        let mut at = 0;
        while at < codes.len() {
            let code = codes[at];
            match code {
                0 => *self = Self::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                7 => self.inverse = true,
                22 => (self.bold, self.dim) = (false, false),
                23 => self.italic = false,
                24 => self.underline = false,
                27 => self.inverse = false,
                30..=37 => self.fg = Some(AnsiColor::Palette(code - 30)),
                90..=97 => self.fg = Some(AnsiColor::Palette(code - 90 + 8)),
                39 => self.fg = None,
                40..=47 => self.bg = Some(AnsiColor::Palette(code - 40)),
                100..=107 => self.bg = Some(AnsiColor::Palette(code - 100 + 8)),
                49 => self.bg = None,
                38 | 48 => {
                    let mut color = None;
                    if at + 2 < codes.len() && codes[at + 1] == 5 {
                        color = Some(AnsiColor::indexed(codes[at + 2]));
                        at += 2;
                    } else if at + 4 < codes.len() && codes[at + 1] == 2 {
                        let part = |n: usize| codes[n].min(255) as u8;
                        color = Some(AnsiColor::Rgb(part(at + 2), part(at + 3), part(at + 4)));
                        at += 4;
                    }
                    if let Some(color) = color {
                        if code == 38 {
                            self.fg = Some(color)
                        } else {
                            self.bg = Some(color)
                        }
                    }
                }
                _ => {}
            }
            at += 1;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiPiece {
    pub text: String,
    pub style: AnsiStyle,
}

/// Applies returns to the start of a line: what is printed after one replaces what was there.
fn with_returns(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            let mut parts: Vec<&str> = line.split('\r').collect();
            while parts.len() > 1 && parts.last() == Some(&"") {
                parts.pop();
            }
            parts.iter().fold(String::new(), |shown, part| {
                let rest: String = shown.chars().skip(part.chars().count()).collect();
                format!("{part}{rest}")
            })
        })
        .collect();
    lines.join("\n")
}

/// Splits terminal output into pieces of one style each.
#[must_use]
pub fn pieces(output: &str) -> Vec<AnsiPiece> {
    let mut pieces = Vec::new();
    let mut style = AnsiStyle::default();
    let mut plain = String::new();
    let push = |plain: &mut String, style: AnsiStyle, pieces: &mut Vec<AnsiPiece>| {
        let shown: String = with_returns(plain)
            .chars()
            .filter(|&character| character == '\n' || character == '\t' || character >= ' ')
            .collect();
        plain.clear();
        if !shown.is_empty() {
            pieces.push(AnsiPiece { text: shown, style });
        }
    };
    let bytes = output.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != 0x1B {
            let next = output[at..]
                .find('\u{1B}')
                .map_or(bytes.len(), |found| at + found);
            plain.push_str(&output[at..next]);
            at = next;
            continue;
        }
        match bytes.get(at + 1) {
            // CSI: ESC [ parameters, then one final letter.
            Some(b'[') => {
                let mut end = at + 2;
                while end < bytes.len() && matches!(bytes[end], b'0'..=b'9' | b';' | b':' | b'?') {
                    end += 1;
                }
                if end < bytes.len() && (b'@'..=b'~').contains(&bytes[end]) {
                    if bytes[end] == b'm' {
                        push(&mut plain, style, &mut pieces);
                        let codes: Vec<usize> = output[at + 2..end]
                            .split([';', ':'])
                            .map(|code| code.parse().unwrap_or(0))
                            .collect();
                        style.apply(&codes);
                    }
                    at = end + 1;
                } else {
                    at += 1;
                }
            }
            // OSC: ESC ] text, ended by BEL or ESC \.
            Some(b']') => {
                let mut end = at + 2;
                while end < bytes.len() && bytes[end] != 0x07 && bytes[end] != 0x1B {
                    end += 1;
                }
                at = match bytes.get(end) {
                    Some(0x07) => end + 1,
                    Some(_) if bytes.get(end + 1) == Some(&b'\\') => end + 2,
                    _ => at + 1,
                };
            }
            Some(b'@'..=b'Z' | b'\\' | b'^' | b'_') => at += 2,
            _ => at += 1,
        }
    }
    push(&mut plain, style, &mut pieces);
    pieces
}

/// The output as one text and the style of each of its ranges, for a `StyledText`.
#[must_use]
pub fn highlighted(output: &str, theme: &Theme) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for piece in pieces(output.trim_matches('\n')) {
        let start = text.len();
        text.push_str(&piece.text);
        let style = piece.style;
        if style == AnsiStyle::default() {
            continue;
        }
        let (fg, bg) = if style.inverse {
            (
                Some(
                    style
                        .bg
                        .map_or(theme.colors.sunken, |color| color.color(theme)),
                ),
                Some(
                    style
                        .fg
                        .map_or(theme.colors.text, |color| color.color(theme)),
                ),
            )
        } else {
            (
                style.fg.map(|color| color.color(theme)),
                style.bg.map(|color| color.color(theme)),
            )
        };
        let color = match (fg, style.dim) {
            (Some(fg), true) => Some(fg.opacity(0.65)),
            (None, true) => Some(theme.colors.secondary),
            (fg, false) => fg,
        };
        highlights.push((
            start..text.len(),
            HighlightStyle {
                color,
                background_color: bg,
                font_weight: style.bold.then_some(FontWeight::BOLD),
                font_style: style.italic.then_some(FontStyle::Italic),
                underline: style.underline.then(|| UnderlineStyle {
                    thickness: px(1.0),
                    color,
                    wavy: false,
                }),
                ..HighlightStyle::default()
            },
        ));
    }
    (text, highlights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_styles_are_read() {
        let found = pieces("plain \u{1B}[1;31mred\u{1B}[0m \u{1B}[38;5;196mx\u{1B}[48;2;1;2;3my");
        assert_eq!(found.len(), 5);
        assert_eq!(found[0].text, "plain ");
        assert_eq!(found[1].style.fg, Some(AnsiColor::Palette(1)));
        assert!(found[1].style.bold);
        assert_eq!(found[2].style, AnsiStyle::default());
        assert_eq!(found[3].style.fg, Some(AnsiColor::Rgb(255, 0, 0)));
        assert_eq!(found[4].style.bg, Some(AnsiColor::Rgb(1, 2, 3)));
    }

    #[test]
    fn other_codes_are_dropped() {
        let found = pieces("a\u{1B}[2Kb\u{1B}]0;title\u{7}c\u{1B}Md\u{8}");
        assert_eq!(
            found
                .iter()
                .map(|piece| piece.text.as_str())
                .collect::<String>(),
            "abcd"
        );
    }

    #[test]
    fn a_redrawn_line_shows_its_last_state() {
        assert_eq!(pieces("10%\r50%\r100%\ndone\r\n")[0].text, "100%\ndone\n");
        assert_eq!(pieces("abcdef\rxy")[0].text, "xycdef");
    }
}
