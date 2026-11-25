# Components: Markdown text and long timelines

2026-11-24. A1 and A5 of the second plan, in React and SwiftUI.

## Markdown (A1)

| | React | SwiftUI |
|---|---|---|
| Parser | `marked` 18 (the user agreed to the dependency) | Foundation's `AttributedString(markdown:)`, full syntax. No dependency |
| How it is drawn | The parser's tokens become React elements | The parsed text is split into blocks by its presentation intents; one view per block |
| Swapping it | `renderText` on `ChatView`, or the `RenderText` context | `.agentText { … }` (an environment value) |

Decisions:

- **Never HTML.** `marked` can produce an HTML string; the kit does not use that. Tokens are turned into elements, so `<script>` or `<img onerror>` in an answer is text on the page. A test holds this.
- **Links** are followed only to `http`, `https` and `mailto`; others are drawn as their text. **Images** are named (`[image: alt]`), not loaded: loading one tells its server that the user is reading.
- **Streaming.** Both parsers treat a code fence that is not closed as code to the end of the text, which is the case that matters while text arrives. A half-written `**bol` is shown as typed until its end arrives. No special handling was needed.
- **Headings** are drawn small (`#` becomes `h3`): an answer is not the page's document.
- **Only the agent's messages and reasoning.** The user's messages stay as written.
- **No code highlighting.** A highlighter is a large dependency with its own choices; an app adds one through the swap point.
- The parse is redone when a message's text changes, so once per chunk for the message that is streaming, not for the others.

Checked with a real Haiku answer in a browser (heading, inline styles, a link, nested and numbered lists, a table, a quote, a code block, literal `<b>`): all drawn right. The SwiftUI view is checked by its block outline and a picture (`.build/bridge-ui-renders/markdown.png`).

Left out: footnotes, definition lists, nested block quotes inside lists in SwiftUI (they come out flat), task-list boxes in SwiftUI.

## Long timelines (A5)

React:

- `Timeline` draws the newest `pageSize` items (200) and a "Show earlier (N more)" button that adds as many again. The count is from the end, so new items do not push older ones out.
- Rows have `content-visibility: auto`, so the browser lays out and paints only those near the screen.
- Measured, drawing to a string: **a session of 5000 items takes 5 ms for the newest 200 and 123 ms for all**.
- Not done: a virtual list that recycles rows while scrolling. It needs measured row heights and a scroll container the kit does not own. Pages are simple and cannot go wrong; an app with a virtual list of its own can pass `pageSize={Infinity}` or use the item components directly.
- Items change in place in the SDK, so rows cannot be skipped by comparing them. With 200 rows and the Markdown parse remembered per text, a chunk redraws cheaply; if this shows in a profile, the SDK needs a per-item change counter.

SwiftUI: the timeline is a `LazyVStack`. Inside the chat's `ScrollView` SwiftUI builds only the rows on screen. Off screen (the picture tests) it still draws everything. **Not measured on screen**: no test opens a window yet (step 5).

## Tests

React 43 (5 new: Markdown, streaming, safety, pages, and a timing that is printed, not judged). The page tests now draw each recorded session twice: as written, to find every message letter for letter, and as Markdown, to find every word. Swift: 2 new.
