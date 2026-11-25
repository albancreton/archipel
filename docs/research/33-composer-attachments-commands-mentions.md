# Components: attachments, commands and mentions in the composer

2026-11-24. A3 and A4 of the second plan, in React and SwiftUI.

## What the harnesses do (checked live, new scenario `prompt.attachments`)

| | Claude | Pi (local model) |
|---|---|---|
| Says it takes | pictures, embedded files | pictures |
| A picture (`image` block) | seen | seen |
| A linked file (`resource_link`) | read | read |
| A file sent with its content (`resource`) | read | read, although not advertised |
| The user's message in the log | keeps every block, the picture's bytes included | the same |

So the kit can draw what the user sent from the log alone. The other side of that: **a picture is stored in the daemon's log in full**, and is sent again to every app that replays the session. Nothing limits that today beyond the composer's 5 MB per file.

## Decisions

- **The composer sends a string when there is only text, and content blocks otherwise.** Apps with a text-only `prompt` handler are not affected until they switch attachments or mentions on. In Swift the action always gets blocks.
- **What can be attached follows what the agent says it takes** (`capabilities.session.prompt`), through `acceptsOf` / `Accepts(agent:)`. Without it, nothing is offered. Pi would take more than it says; the kit follows what it says.
- **A picture is an `image` block, any other file a `resource` with its content** (text when it is text, base64 otherwise). The file is read in the app, so this works for a daemon on another machine, where a path would mean nothing.
- **Mentions come from a function the app gives.** The kit has no file listing and the daemon has none either: which files exist, and which to hide, is the app's knowledge. A chosen mention is `@name` in the text plus a `resource_link`; a mention the user deletes from the text is not sent.
- **Commands** are the agent's `availableCommands`. The menu opens while the draft is `/name` and nothing else, since a command starts a message.
- **A picture in the user's message is drawn only from its own bytes** (a `data:` address with a checked type), never from a web address.
- SwiftUI: no paste of pictures, and a mention is found only at the end of the text (the field does not expose the caret).

## Tests

React 46: what is being typed, what is sent, what is offered, and the user's message with attachments (including a type crafted to break out of the attribute). Swift: the same functions, and a picture of the message. Menus and file pickers are not clicked by any test yet (step 5). Nothing was tried in a browser by hand this time; the live scenario covers the wire.
