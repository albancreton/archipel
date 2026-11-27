---
name: archipel component kit
description: A coding-agent session drawn as the terminal's own surface, on Minitel's palette.
colors:
  fg: "#E6E9EF"
  muted: "#A9B0BF"
  bg: "#282C34"
  soft: "#30343E"
  raised: "#3A3F4B"
  sunken: "#16191D"
  line: "#444A58"
  edge: "#868EA2"
  accent: "#8799DE"
  on-accent: "#16191D"
  ok: "#59C780"
  bad: "#F2777A"
  run: "#BA8AF5"
  attn: "#F2A640"
  light-fg: "#16191D"
  light-muted: "#545B6B"
  light-bg: "#F3F4F7"
  light-soft: "#FFFFFF"
  light-raised: "#E4E7EE"
  light-sunken: "#EAECF1"
  light-line: "#D3D7E0"
  light-edge: "#868EA2"
  light-accent: "#3F55B0"
  light-on-accent: "#FFFFFF"
  light-ok: "#1D7A45"
  light-bad: "#B33833"
  light-run: "#7A3FC9"
  light-attn: "#8A5300"
typography:
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "18px"
    fontWeight: 600
    lineHeight: 1.35
  title-request:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    lineHeight: 1.4
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.6
  secondary:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.5
  mono:
    fontFamily: "ui-monospace, SF Mono, SFMono-Regular, Menlo, Consolas, monospace"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.55
  meta:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "0.05em"
  button:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "15px"
    fontWeight: 500
    lineHeight: 1.4
rounded:
  code-inline: "5px"
  hit: "6px"
  control: "8px"
  card: "10px"
  bubble: "14px"
  pill: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  column: "820px"
components:
  button:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.fg}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "7px 14px"
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "7px 14px"
  button-send:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.card}"
    padding: "12px 20px"
  button-danger:
    backgroundColor: "{colors.bad}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.control}"
    padding: "7px 14px"
  input:
    backgroundColor: "{colors.sunken}"
    textColor: "{colors.fg}"
    rounded: "{rounded.control}"
    padding: "8px 12px"
  composer:
    backgroundColor: "{colors.sunken}"
    textColor: "{colors.fg}"
    rounded: "{rounded.card}"
    padding: "12px 14px"
  card:
    backgroundColor: "{colors.soft}"
    textColor: "{colors.fg}"
    rounded: "{rounded.card}"
    padding: "12px 16px"
  user-message:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.fg}"
    rounded: "{rounded.bubble}"
    padding: "10px 16px"
  code-block:
    backgroundColor: "{colors.sunken}"
    textColor: "{colors.fg}"
    typography: "{typography.mono}"
    rounded: "{rounded.control}"
    padding: "12px 14px"
  chip:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.fg}"
    typography: "{typography.meta}"
    rounded: "{rounded.control}"
    padding: "3px 10px"
  badge:
    typography: "{typography.meta}"
    rounded: "{rounded.pill}"
    padding: "1px 10px"
  session-row:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.fg}"
    rounded: "{rounded.card}"
    padding: "10px 12px"
  session-row-selected:
    backgroundColor: "{colors.soft}"
    textColor: "{colors.fg}"
    rounded: "{rounded.card}"
---

# Design System: archipel component kit

This file records the Minitel theme as shipped for React in `react/styles.css` (next to this file) and for SwiftUI in `swift/` (`ArchipelTheme.minitel()`), with the same values; pixels in one are points in the other. What each component has to draw, and the behaviour rules every set keeps, live in `ui/COMPONENTS.md`; this file does not repeat them. A SwiftUI kit built to the same world takes its values from the frontmatter and its rules from here.

## Overview

**Creative North Star: "The Terminal's Other Half"**

A session is drawn on the same ground as the terminal beside it. Minitel's terminal background (#282C34) is the kit's background, so chat and terminal read as one window, and everything the agent does sits on that ground as quiet, bordered cards one step lighter. The world is calm by default: neutrals carry almost everything, colour appears only to say something (running, done, failed, waiting on you).

Readability decides every size. People follow these runs for hours, so nothing is small: body text is 16px at 1.6, secondary text and code are 15px, and the smallest text anywhere (meta, labels, badges) is 14px. Density comes from tight vertical rhythm and one 820px column, never from shrinking type.

Dark is the default identity. A light theme built from the same hues ships behind `data-arc-theme="light"` on the kit's root or any ancestor; it swaps the token values and nothing else. Every colour is a CSS variable on `.arc-chat`, `.arc-sessions` and `.arc-settings`, so an app restyles the kit by overriding tokens, not rules.

**Key Characteristics:**
- Ground is #282C34, shared with the terminal; cards are one tonal step up, code one step down.
- Four signal colours with one meaning each; amber is reserved for "waiting for the user".
- No text under 14px; body 16/1.6.
- Flat: 1px quiet borders and tonal steps, one shadow in the whole kit (the floating menu).
- Soft 10px corners on cards, 8px on controls, pills only for status badges.
- Two themes from one set of fourteen roles.

## Colors

Cool slate neutrals from Minitel's Theme.swift, one periwinkle accent for action, and four signal hues that each mean one thing. Values are in the frontmatter; the `light-*` keys are the same roles in the light theme.

### Primary
- **Periwinkle** (accent): the action colour. The default choice in a request, Send, Fork, links, the focus ring (2px outline, 2px offset), the caret, the text-selection tint (40%), and the highlighted row of the command menu (22% tint). Text on it is **Ink** (on-accent), dark in the dark theme and white in light.

### Secondary (signals)
- **Amethyst** (run): running, and the selected session's border. Pulses on a running badge's dot when motion is allowed. Also the dot marking the in-progress plan step.
- **Amber** (attn): waiting for the user, and nothing else. A pending approval or question gets a 2px amber border on an 8% amber tint; "open elsewhere" and notices that need an action use an amber 1px border; a context meter above its threshold fills amber.
- **Signal Green** (ok): completed, exit 0, added lines.
- **Coral** (bad): failed or cancelled, errors, removed lines, the destructive confirm button.

### Neutral
- **Ground** (bg): the page and the terminal behind it. Session rows rest on it.
- **Card Slate** (soft): every bordered card (plan, a request that waits, a question, session message, notice, activity). A tool call is not a card, nor an approval that has its answer: they are quiet lines, so that the agent's own text is the strongest thing in a turn. A subagent is not a card either: its row carries the chevron, label, title and badge, and its timeline hangs below on a 1px edge-colour guide line indented 7px, so cards never nest, hovered and selected session rows, table headers.
- **Raised Slate** (raised): the user's message, buttons, chips, the command menu, inline code.
- **Well** (sunken): anything the user reads as raw output or types into: code blocks, terminal output, diffs, inputs, the composer.
- **Hairline** (line): every 1px border and divider, the meter's track.
- **Edge** (edge): the stronger border on hover, the blockquote rule, scrollbar thumb, the meter's normal fill. Same value in both themes.
- **Text** (fg) and **Muted** (muted): primary and secondary text. Muted carries meta, labels, thoughts, compaction, answered requests and line numbers.

### Terminal colours
ANSI 0 to 15 are their own tokens (`--arc-ansi-N` in `styles.css`, dark and light sets). They reuse the signal hues for red, green, yellow, blue and magenta, add a cyan, and give brighter variants for 8 to 15. ANSI text is drawn with these, never with arbitrary RGB, so terminal output stays in the palette.

### Named Rules
**The Amber Means You Rule.** Amber (attn) appears only where the user is being waited on: a pending request, a session waiting for an answer, a session open in another window, a notice that needs an action, a nearly full context. Never use it for warnings in general, decoration or emphasis. If a screen shows amber, the user has something to do.

**The One Meaning Rule.** Purple is running (and the selected session), green is done, coral is failed, periwinkle is action. A hue never borrows another's meaning, and none is used decoratively.

**The Tinted Badge Rule.** A status badge is a pill on a 12% tint of its tone, with a 7px dot in the full tone and text mixed 70% tone toward fg. That keeps status text readable on both themes without saturated fills.

**The Shared Ground Rule.** The kit's background is the terminal's background. Do not lighten, gradient or texture the ground; depth comes from cards on it.

## Typography

**Body Font:** the platform's system sans (-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif)
**Mono Font:** the platform's system mono (ui-monospace, SF Mono, Menlo, Consolas)

**Character:** Native and unbranded, so the kit sits inside any app without a type clash. Numbers are tabular throughout (`font-variant-numeric: tabular-nums`) so costs, counts and line numbers align.

### Hierarchy
- **Title** (600, 18px, 1.35): the session title in the status bar, one line with ellipsis.
- **Request title** (600, 17px, 1.4): the question or the action an approval asks about.
- **Body** (400, 16px, 1.6): agent and user messages, session titles in the list (500 weight). Markdown headings scale from it (h3 1.2em, h4 1.08em, others 1em, weight 650).
- **Secondary** (400, 15px): thoughts, compaction, notices, activity, settings, buttons (500), Markdown tables.
- **Mono** (400, 15px, 1.5 to 1.55): tool titles, commands, code blocks, diffs, menu commands. Inline code is max(14px, 0.9em).
- **Meta** (400, 14px): usage and cost, session meta, menu detail, chips, badges (500), diff file headers.
- **Label** (600, 14px, 0.05em, uppercase, muted): names the kind of a record on the same row as its content (tool kind, Plan, Permission, Question, Subagent, Tasks, a setting's name) and the folder heading in the session list.

### Named Rules
**The Fourteen Floor Rule.** No text is ever under 14px, in either kit. Secondary information is made quieter with muted colour, not with a smaller size.

**The Label Names Data Rule.** An uppercase label always names what the record is, inline with it; it is never a decorative kicker over a heading.

## Layout

The chat is one column, 820px at most, centred, padded 20px 24px (16px under 600px wide). Top to bottom: status bar (divided by a 1px line, 14px below), timeline, then around-the-timeline parts, then the composer (1px line above, 14px padding). Items in the timeline are 14px apart; the chat's own parts are 16px apart.

Spacing steps are 4, 8, 12, 16 and 24px, with 10 and 14 used inside rows. Cards pad 12px 16px (10px 12px on narrow screens). User messages sit on the right, at most 85% of the column (92% narrow); everything the agent does runs full width on the left.

Long content scrolls inside its own box rather than the page: code blocks stop at 320px, diffs at 460px, the command menu at 300px. Rows off screen are not laid out (`content-visibility: auto`), which keeps 5000-item sessions fast.

The session list is a vertical stack with 16px padding, 18px between folder groups and 4px between rows.

## Elevation & Depth

Flat, with tonal layering. Depth is three steps around the ground: **sunken** for what is raw output or input, **ground**, **soft** for cards, **raised** for what the user owns or can press. Borders are 1px in Hairline and turn Edge on hover.

### Shadow Vocabulary
- **Floating menu** (`box-shadow: 0 12px 32px rgba(0, 0, 0, 0.35)`): the composer's command menu, the only element that floats over content.

### Named Rules
**The One Float Rule.** Only something that overlaps content gets a shadow. Cards, buttons and messages are flat.

## Shapes

Gently rounded and consistent. Cards, notices, the composer and session rows use 10px (`--arc-radius`, the one radius an app may override); controls, code blocks, diffs and chips use 8px; small hit areas (summaries, menu rows) 6px; inline code 5px. The user's message is a 14px bubble with its bottom-right corner cut to 4px, pointing toward the user's side. Status badges and the meter are the only fully round shapes.

Icons are authored SVG on a 16 by 16 grid, drawn at 16px with a 1.4 stroke, round caps and joins, no fill, in `currentColor`: the rename and delete actions, the plan step marks, and the disclosure chevron (masked so it takes the text colour, turning 90 degrees in 120ms when open).

## Components

### Buttons
- **Shape:** 8px corners, 1px Hairline border.
- **Default:** Raised Slate with Text, 15px at weight 500, 7px 14px.
- **Primary:** Periwinkle with Ink at weight 600: the first choice of a request, Send, Fork. Hover mixes 15% fg into the accent.
- **Send:** primary at 12px 20px and 10px corners, matching the composer's height.
- **Danger:** Coral fill with Ink, only in the delete confirmation.
- **Quiet:** "earlier items" is transparent with a Muted 14px label; diff folds are underlined accent text.
- **Hover / Focus:** border turns Edge in 120ms; focus is a 2px accent outline with 2px offset. Disabled is 50% opacity.

### Badges and Chips
- **Badge:** see The Tinted Badge Rule. Tones: muted (neutral), run, ok, bad, attn.
- **Chip:** Raised Slate, 8px corners, 1px border, 14px, for attachments and background tasks (mono, on Well).

### Cards / Containers
- **Corner Style:** 10px.
- **Background:** Card Slate, with Well for any code or output inside.
- **Border:** 1px Hairline.
- **Internal Padding:** 12px 16px.
- **Pending request:** 2px amber border on an 8% amber tint (padding reduced by 1px so the box does not move).

### Inputs / Fields
- **Style:** Well background, 1px Hairline, 8px corners, 8px 12px; the composer at 10px corners and 12px 14px, at least 52px tall. Placeholder is Muted.
- **Focus:** the shared accent outline. Hover turns the border Edge.

### Navigation (session list)
- Rows rest on the ground with a transparent border; hover and focus lift them to Card Slate; the selected row is Card Slate with an Amethyst border. Rename and delete appear on hover or focus (always on touch screens) as 32px icon buttons over a gradient into the row colour.

### Timeline items (signature)
- **Tool call:** one muted 15px line, no card: chevron, label, mono title (ellipsis), and a badge unless it completed. Its output, in a Well code block, sits under the line, in from the chevron, while the call runs or when the line is pressed; the title turns to the text colour while it runs and on hover. Finished calls in a row fold into one line of the same kind, "N tool calls" (and " · M failed" in Coral), which opens to them. An approval that has its answer is such a line too, without a chevron: label, title, answer. Diffs are tables in mono 15px with 16% green or coral row tints, signs at 70% tone, muted line numbers.
- **Thought and compaction:** muted 15px behind a chevron, no card.
- **Plan:** steps with authored marks; the in-progress step is bold with a purple dot; done steps are muted and struck through in Edge.

## Do's and Don'ts

### Do:
- **Do** keep the ground at #282C34 in the dark theme so the kit and the terminal read as one surface.
- **Do** keep every text size at 14px or more; reach for muted colour, not smaller type, to quiet something.
- **Do** reserve amber for what waits on the user.
- **Do** draw status as a tinted pill with a dot (12% tint, 70% text mix).
- **Do** put raw output and anything typed into on the sunken Well, and cards on Card Slate.
- **Do** use authored 16px SVG icons with a 1.4 round stroke in currentColor.
- **Do** read every colour from a token so the light theme and app overrides follow.

### Don't:
- **Don't** use amber for generic warnings, highlights or decoration.
- **Don't** give purple, green, coral or periwinkle a second meaning.
- **Don't** add shadows to cards, buttons or messages; only the floating menu has one.
- **Don't** make the kit a light chat app by default (white page, blue bubbles, tiny grey metadata); light is opt-in.
- **Don't** fill status badges with saturated colour or put status text in the full tone on a tint.
- **Don't** use icon fonts or glyph characters as icons.
