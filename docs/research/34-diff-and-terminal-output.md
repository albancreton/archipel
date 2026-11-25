# Components: diffs and terminal output

2026-11-24. A7 and A8 of the second plan, in React and SwiftUI. No new dependency in either kit.

## Diff (A7)

- The patch the agent sends (unified, as `git diff` prints) is read into files and rows. Each file shows its path, what happened to it, and `+added −removed`; each row its old and new line number.
- **Unchanged lines are folded** when a run of them would hide more than three lines, keeping three next to each change. A fold opens in place. Adapters differ in how much context they send; a whole-file diff of a one-line change is now a few rows.
- A line of content that looks like a header (`+--- x`) stays content: headers are only read outside a hunk, or as a `---` line directly followed by `+++`.
- Text that is no patch is shown as it came.
- **No syntax highlighting, no word-level marks.** Both need a tokenizer per language; that is a dependency an app can add.

## Terminal output (A8)

- A small reader for the colour and style codes (SGR): the 16 colours, bright ones, the 256 table, exact colours, bold, dim, italic, underline, inverse.
- Every other control code is dropped (cursor moves, window titles, bells), so no stray characters show.
- A return to the start of a line replaces what was there, so a progress bar shows its last state, not every step.
- **It is not a terminal emulator.** Full-screen programs (an editor, `top`) will not draw right; they would need a screen model. An app that needs that mounts a real emulator in place of this component.
- React: the 16 colours are CSS variables with light and dark values. SwiftUI: the system's colours.

Not checked against a real coloured command through a harness: the adapters run commands without a terminal, so most programs print no colours there. The parser is tested on written sequences.

## Tests

React 49 (3 new). Swift 11 in the kit (2 new), with pictures `diff.png` and `terminal-colours.png`.
