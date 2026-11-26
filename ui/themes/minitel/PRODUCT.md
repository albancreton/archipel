# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- **App builders**: developers who embed archipel sessions in their own apps (Electron, browser, React Native) with `@archipel/react`. Minitel-style tools are the first real consumer.
- **End users**: developers who watch and steer coding agents (Claude Code, Pi) for long stretches, often beside a terminal. They read the agent's text, tool calls, diffs and terminal output, and they answer approvals and questions.

## Product Purpose

The component kit draws one agent session (messages, reasoning, tool calls, approvals, questions, plans, subagents) and the session list. An app can use it as it is or restyle it. It succeeds when someone can follow a long agent run at a glance and act on what needs them without hunting for it.

## Positioning

The kit draws only what the daemon's state holds, so it stays correct across harnesses, extensions (`x.<namespace>.<name>` kinds) and several apps attached at once. The rules are in `ui/COMPONENTS.md`.

## Operating Context

- Used for hours at a time, in a window next to terminals. Minitel's terminal background is `#282C34`, and the kit's main background matches it so the two read as one surface.
- The content is dense and technical: Markdown, code, diffs, ANSI terminal output, long sessions of up to 5000 items.

## Capabilities and Constraints

- A theme, not a kit: it restyles the component kits in `ui/react` and `ui/swift` without changing what they draw. For React it is one stylesheet (`react/styles.css`) used in place of the kit's own; every class starts with `arc-`, and the colours are CSS variables an app may override. The SwiftUI version goes in `swift/`.
- Nothing small: body text at 16px, and no text under 14px, labels, badges, code and diffs included.
- Rules from `ui/COMPONENTS.md` stay in force: draw from the current state, never drop an unknown kind, Markdown is never HTML, ask before deleting anything, and only offer what the agent can do.
- Two themes. Dark is the default identity, built on Minitel's palette. A light theme derived from the same palette still ships.
- The SwiftUI kit (`ui/swift`) follows the React kit later.

## Brand Commitments

- Palette based on the Minitel Mac app (`../minitel/mac/Sources/Minitel/Theme.swift`) and the Figma playground (node 1631-2). `#282C34` is the main background.

## Product Principles

1. Readability first: nothing small. People read this for hours.
2. What needs the user stands out at once. Everything else stays calm.
3. It sits beside the terminal: the same surface and the same colour language.
4. Restylable by apps: tokens, not hard-coded values.

## Accessibility & Inclusion

- No small text: the body reads comfortably, and secondary text stays large enough to read without effort.
- Text contrast meets WCAG AA on every surface in both themes.
