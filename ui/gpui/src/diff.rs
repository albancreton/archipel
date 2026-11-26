//! A change to one or more files: per file, how much changed and the changed lines with their
//! numbers. Long unchanged parts are folded. A patch this cannot read is shown as it is.

use crate::memory;
use crate::parts::{code_block, link};
use crate::theme::theme;
use gpui::{App, Div, SharedString, div, prelude::*, px, relative};
use serde_json::Value;

/// One row of a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    Add(String, usize),
    Remove(String, usize),
    /// An unchanged line, with its old and new numbers.
    Context(String, usize, usize),
    Hunk(String),
    /// Unchanged lines that are not shown until the user asks.
    Fold(Vec<DiffRow>),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub rows: Vec<DiffRow>,
}

/// The two line numbers of a hunk header: `@@ -12,5 +14,6 @@`.
fn hunk_start(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(' ')?;
    let (new, _) = rest.strip_prefix('+')?.split_once(" @@")?;
    let number = |part: &str| part.split(',').next()?.parse().ok();
    Some((number(old)?, number(new)?))
}

/// Reads a unified patch (what `git diff` prints). Text that is no patch gives no files.
#[must_use]
pub fn parse_patch(patch: &str) -> Vec<DiffFile> {
    fn name(line: &str) -> String {
        let name = line.get(4..).unwrap_or("");
        let name = name.split('\t').next().unwrap_or(name);
        name.strip_prefix("a/")
            .or_else(|| name.strip_prefix("b/"))
            .unwrap_or(name)
            .to_owned()
    }
    let mut files: Vec<DiffFile> = Vec::new();
    let (mut old_line, mut new_line, mut in_hunk, mut has_file) = (0, 0, false, false);
    let mut lines: Vec<&str> = patch.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        at += 1;
        if line.starts_with("diff --git ") {
            (has_file, in_hunk) = (false, false);
            continue;
        }
        if line.starts_with("--- ") && lines.get(at).is_some_and(|next| next.starts_with("+++ ")) {
            let to = name(lines[at]);
            let path = if to == "/dev/null" || to == "dev/null" {
                name(line)
            } else {
                to
            };
            files.push(DiffFile {
                path,
                ..DiffFile::default()
            });
            (has_file, in_hunk) = (true, false);
            at += 1;
            continue;
        }
        if let Some((old, new)) = hunk_start(line) {
            if !has_file {
                files.push(DiffFile::default());
                has_file = true;
            }
            (old_line, new_line, in_hunk) = (old, new, true);
            if let Some(file) = files.last_mut() {
                file.rows.push(DiffRow::Hunk(line.to_owned()));
            }
            continue;
        }
        // Index lines, modes, and text before the first hunk.
        if !has_file || !in_hunk {
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        let text = line.get(1..).unwrap_or("").to_owned();
        if line.starts_with('+') {
            file.rows.push(DiffRow::Add(text, new_line));
            new_line += 1;
            file.added += 1;
        } else if line.starts_with('-') {
            file.rows.push(DiffRow::Remove(text, old_line));
            old_line += 1;
            file.removed += 1;
        } else if line.starts_with('\\') {
            // "\ No newline at end of file"
        } else {
            file.rows.push(DiffRow::Context(text, old_line, new_line));
            old_line += 1;
            new_line += 1;
        }
    }
    files
}

/// Folds runs of unchanged lines, keeping `keep` of them next to each change. A run is folded
/// only when that hides more than three lines; fewer are not worth a click.
#[must_use]
pub fn fold_context(rows: &[DiffRow], keep: usize) -> Vec<DiffRow> {
    let is_context = |row: &DiffRow| matches!(row, DiffRow::Context(..));
    let mut folded: Vec<DiffRow> = Vec::new();
    let mut at = 0;
    while at < rows.len() {
        if !is_context(&rows[at]) {
            folded.push(rows[at].clone());
            at += 1;
            continue;
        }
        let mut end = at;
        while end < rows.len() && is_context(&rows[end]) {
            end += 1;
        }
        let run = &rows[at..end];
        // No change before the run (the start, or a hunk header) or after it: keep that side short.
        let before = match folded.last() {
            Some(DiffRow::Hunk(_)) | None => 0,
            Some(_) => keep,
        };
        let after = if end < rows.len() { keep } else { 0 };
        if run.len() > before + after + 3 {
            folded.extend_from_slice(&run[..before]);
            folded.push(DiffRow::Fold(run[before..run.len() - after].to_vec()));
            folded.extend_from_slice(&run[run.len() - after..]);
        } else {
            folded.extend_from_slice(run);
        }
        at = end;
    }
    folded
}

fn rows(key: &str, rows: &[DiffRow], cx: &App) -> Div {
    let theme = theme(cx);
    let number = |value: Option<usize>| {
        div()
            .flex_none()
            .w(px(theme.text.code * 3.0))
            .text_color(theme.colors.secondary)
            .flex()
            .justify_end()
            .child(SharedString::from(
                value.map(|n| n.to_string()).unwrap_or_default(),
            ))
    };
    let numbered = |old: Option<usize>, new: Option<usize>, sign: &'static str, text: &str| {
        div()
            .flex()
            .items_start()
            .gap(px(6.0))
            .w_full()
            .px(px(4.0))
            .child(number(old))
            .child(number(new))
            .child(div().flex_none().w(px(theme.text.code * 0.7)).child(sign))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(SharedString::from(text.to_owned())),
            )
    };
    let mut column = div().flex().flex_col().w_full();
    for (index, row) in rows.iter().enumerate() {
        column = column.child(match row {
            DiffRow::Hunk(text) => div()
                .w_full()
                .px(px(8.0))
                .py(px(2.0))
                .bg(theme.colors.line.opacity(0.4))
                .text_color(theme.colors.secondary)
                .child(SharedString::from(text.clone())),
            DiffRow::Add(text, new) => {
                numbered(None, Some(*new), "+", text).bg(theme.colors.ok.opacity(0.16))
            }
            DiffRow::Remove(text, old) => {
                numbered(Some(*old), None, "-", text).bg(theme.colors.bad.opacity(0.16))
            }
            DiffRow::Context(text, old, new) => numbered(Some(*old), Some(*new), " ", text),
            DiffRow::Fold(inner) => {
                let key = format!("{key}/{index}");
                if memory::flag(cx, &key, false) {
                    self::rows(&key, inner, cx)
                } else {
                    let flag = key.clone();
                    div().px(px(8.0)).child(
                        link(key, &format!("Show {} unchanged lines", inner.len()), cx)
                            .text_size(px(theme.text.code))
                            .on_click(move |_, _, cx| memory::set_flag(cx, &flag, true)),
                    )
                }
            }
        });
    }
    column
}

/// A diff block of a tool call: `{ patch: { text }, changes: [{ path, operation }] }`. `key` is
/// where the kit remembers which folds were opened.
pub fn diff(key: &str, block: &Value, cx: &App) -> Div {
    let theme = theme(cx);
    let patch = block
        .pointer("/patch/text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let files = parse_patch(patch);
    let changes: &[Value] = block
        .get("changes")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let text = |change: &Value, field: &str| {
        change
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let mut column = div().flex().flex_col().gap(px(6.0)).w_full();
    if files.is_empty() {
        for change in changes {
            column = column.child(
                div()
                    .text_size(px(theme.text.meta))
                    .text_color(theme.colors.secondary)
                    .child(SharedString::from(format!(
                        "{} {}",
                        text(change, "operation"),
                        text(change, "path")
                    ))),
            );
        }
        if !patch.is_empty() {
            column = column.child(code_block(patch, cx));
        }
    }
    for (index, file) in files.iter().enumerate() {
        let operation = changes
            .iter()
            .find(|change| text(change, "path") == file.path)
            .map(|change| text(change, "operation"));
        let title = [operation, Some(file.path.clone())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        column = column.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .rounded(px(theme.control_radius))
                .bg(theme.colors.sunken)
                .border_1()
                .border_color(theme.colors.line)
                .overflow_hidden()
                .font_family(theme.text.mono_family.clone())
                .text_size(px(theme.text.code))
                .line_height(relative(1.45))
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .px(px(8.0))
                        .py(px(4.0))
                        .border_b_1()
                        .border_color(theme.colors.line)
                        .text_color(theme.colors.secondary)
                        .child(div().min_w_0().truncate().child(SharedString::from(title)))
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.colors.ok)
                                .child(SharedString::from(format!("+{}", file.added))),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.colors.bad)
                                .child(SharedString::from(format!("−{}", file.removed))),
                        ),
                )
                .child(rows(
                    &format!("{key}/{index}"),
                    &fold_context(&file.rows, 3),
                    cx,
                )),
        );
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n one\n-two\n+2\n three\n\\ No newline at end of file\n";

    #[test]
    fn a_patch_is_read_with_its_line_numbers() {
        let files = parse_patch(PATCH);
        assert_eq!(files.len(), 1);
        assert_eq!(
            (files[0].path.as_str(), files[0].added, files[0].removed),
            ("src/a.rs", 1, 1)
        );
        assert_eq!(files[0].rows[2], DiffRow::Remove("two".to_owned(), 2));
        assert_eq!(files[0].rows[3], DiffRow::Add("2".to_owned(), 2));
        assert_eq!(files[0].rows[4], DiffRow::Context("three".to_owned(), 3, 3));
        assert!(parse_patch("not a patch").is_empty());
    }

    #[test]
    fn long_unchanged_runs_fold() {
        let mut rows = vec![
            DiffRow::Hunk("@@".to_owned()),
            DiffRow::Add("new".to_owned(), 1),
        ];
        rows.extend((0..12).map(|n| DiffRow::Context(format!("{n}"), n + 1, n + 2)));
        rows.push(DiffRow::Remove("old".to_owned(), 13));
        let folded = fold_context(&rows, 3);
        // Three kept after the addition, six folded, three kept before the removal.
        assert_eq!(folded.len(), 2 + 3 + 1 + 3 + 1);
        assert!(matches!(&folded[5], DiffRow::Fold(inner) if inner.len() == 6));
    }
}
