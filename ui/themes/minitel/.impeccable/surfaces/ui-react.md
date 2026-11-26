---
version: 1
slug: "ui-react"
primary_target: "ui/react"
related_targets: []
---

# Surface: @archipel/react component kit

Mode: Operate. Scope: the Minitel theme for the React kit, `ui/themes/minitel/react/styles.css`, over the components in `ui/react/src/`; the catalogue (`pnpm catalog:minitel` in `ui/react`, `demo/out/catalog-minitel.html`) is how it is reviewed.

## Direction contract

THESIS: A coding-agent session drawn as the terminal's own surface: Minitel's #282C34 ground continues straight into the chat, so the kit and the terminal beside it read as one window. Refuses the generic light chat-app look (white page, blue bubbles, tiny grey metadata).

OWN-WORLD: Minitel palette from Theme.swift and the Figma swatches: neutrals #16191D sunken (code, diffs, fields), #282C34 ground, #30343E cards, #3A3F4B raised (user message, buttons, menus), #868EA2 edge. Periwinkle #8799DE accent with dark ink, purple #BA8AF5 for running and the selected session, amber #F2A640 for anything waiting on the user, green #59C780 ok, coral #F2777A errors. 1px quiet borders, 10px radius, tinted pill badges with a dot. A light theme from the same hues behind data-arc-theme="light".

STORY: The user follows a long agent run at a glance, reads text and code without effort, and finds what needs them (amber) at once.

FIRST VIEWPORT: Status bar (18px title, meter, cost, badge), then the timeline at 16px/1.6 in an 820px column, the composer at the bottom with a periwinkle Send.

FORM: The existing component structure, set by ui/COMPONENTS.md; only the visual world changes. seed: none (pinned) — "we'll use a color palette based on the Minitel app … where #282C34 is the main background color. We need to focus on reability, avoiding small text".

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
