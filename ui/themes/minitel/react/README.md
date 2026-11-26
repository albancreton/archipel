# Minitel theme for `@archipel/react`

A stylesheet for the React kit's components, on the Minitel palette: `#282C34`, the terminal's own background, is the main one, so a session reads as one surface with the terminal beside it. It is used in place of the kit's own stylesheet; the components and their `arc-` classes are the same.

```tsx
import { ChatView, useSession } from "@archipel/react";
import "../ui/themes/minitel/react/styles.css"; // instead of "@archipel/react/styles.css"
```

- Dark is the default. `data-arc-theme="light"` on the kit's root or on any ancestor switches to the light version of the same colours.
- No text is under 14px: body 16/1.6, code and diffs 15, labels, badges and metadata 14.
- Amber means only "waiting for the user" (a pending approval or question, `needs you`, a nearly full context). Purple means running, and the selected session.
- The colours are CSS variables on `.arc-chat`, `.arc-sessions` and `.arc-settings` (`--arc-bg`, `--arc-accent`, `--arc-attn`, …, and `--arc-ansi-0` to `--arc-ansi-15`); override them like the kit's own.

To see every component with it, in `ui/react`: `pnpm catalog:minitel` writes `demo/out/catalog-minitel.html`, and `pnpm demo:minitel` writes the recorded sessions to `demo/out/minitel/`.

The look, its tokens and its rules: [`../DESIGN.md`](../DESIGN.md). Who it is for: [`../PRODUCT.md`](../PRODUCT.md).
