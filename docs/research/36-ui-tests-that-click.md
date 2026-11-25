# UI tests that click

2026-11-24. Step 5 of the second plan, first half (tier T4 of note 21): tests that use the two kits the way a person does. Until now the kits were tested as functions and as drawn output.

## React: a real browser

`pnpm test:browser` in `ui/react`, 13 tests, about three seconds. Playwright drives a headless Chromium; the tests are plain `node:test` files like the others.

- **`test/browser/parts.test.ts`** (8 tests) loads one part of the kit at a time with written data (`parts.page.tsx`) and checks what the user sees and what the app is asked to do. The session list (open, rename with Escape and Return, delete with its question, a refusal shown on the row, "Show more"); the composer (Return and Shift+Return, the command menu with arrows, Return, Escape and a click, mentions with Tab, the file chooser, a pasted and a dropped picture, a file that is too large, an agent that takes only pictures, a refused message that comes back, Stop); the settings bar (a choice, a switch, a refusal); an approval and a question's form; "Show earlier" on a session of 450 messages; the folds of thinking and of a diff; the fork button; "Try again".
- **`test/browser/live.test.ts`** (5 tests) goes the whole way: the demo page, the daemon program on a WebSocket, and the fake agent replaying a recorded Claude session. A turn with an approval that the user allows; a question answered with its form; Stop on a running turn; a session from the agent's own history opened with its history; and the daemon stopped and started again under the page, which says "Reconnecting…" and then carries on with what it had.

The demo page got a "New session" button for this (`&cwd=/some/folder` in its address).

Why Playwright and not a DOM imitation (jsdom): focus, the caret, the file chooser, paste, drop and a real WebSocket are what these tests are about, and an imitation has none of them faithfully. The cost is one dev dependency and a browser download of 100 MB (`npx playwright install chromium-headless-shell`, once per machine).

## SwiftUI: pressed inside the test process

`swift test` now runs 8 click tests in the kit (20 in all, under four seconds). `Tests/ArchipelUITests/Screen.swift` puts a view in a window that is never shown, finds its controls through accessibility by their label, presses them, and types into its text fields through the field's own editor.

How this was found: Apple's UI tests (XCUITest) need an app bundle and an Xcode project, which a Swift package does not have. Reading the accessibility tree from inside the process needs neither, and no permission. SwiftUI only describes its views to accessibility once something has asked; setting `AXEnhancedUserInterface` on the application object does that. A `List` builds its rows only when drawn, so the helper draws the window into memory, and it looks inside the row views because the rows do not report their content.

Covered: an approval, a question (typing and Skip), the composer (sending, the command menu, mentions, a refused message, Stop, Attach offered or not), a switch in the settings bar with a refusal, the fork button, the folds of a diff and of thinking, "Try again", and the session list's rows, headings and "Show more".

**Not reachable this way:** menus (the pickers of the settings bar and of a question, a row's Rename and Delete), alerts and the delete question, the file dialog, drags, and choosing a row in the list. They run their own event loop. They are still only drawn. The demo app as a whole is still only compiled.

## What the tests found

1. **SwiftUI composer: Return with the file menu open sent the message instead of taking the file.** The text field keeps the handlers it was given until its own text changes, and the files to mention arrive after that, so the handler still saw an empty menu. The handlers now ask for the menu when the key is pressed. The same would have hit Tab and the arrows. The command menu was not affected (it depends only on the text). Seen in the test process, not on a screen.
2. **React: "Try again" on an agent notice passed the click event to the app's function.** Harmless for a function without parameters, wrong for one with an optional one. Fixed.
3. React: the composer's menus open above it, so a composer at the very top of a page puts them out of view. Not changed: a composer sits at the bottom of a chat. An app that puts it elsewhere needs its own style for `.arc-menu`.

## Not done

- Nothing measures time: how long a session of several thousand items takes to appear in a browser or in SwiftUI is still unmeasured.
- The React tests run in Chromium only. Safari and Firefox were not tried.
- No test looks at layout (overlap, clipping, dark mode).
- SwiftUI on iOS is compiled for the platform by nobody: the package says iOS 17, no test builds it.
