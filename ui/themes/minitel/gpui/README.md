# Minitel theme for the GPUI kit

`archipel-gpui-minitel`: the look recorded in [`../DESIGN.md`](../DESIGN.md), for the components of `archipel-gpui`. The same colours, sizes and rules as [`../react/styles.css`](../react/styles.css) and the SwiftUI version. `#282C34`, the terminal's own background, is the main one.

```rust
archipel_gpui::init(cx);
archipel_gpui::set_theme(cx, archipel_gpui_minitel::minitel(true));   // dark, as in the app
archipel_gpui::set_theme(cx, archipel_gpui_minitel::minitel(false));  // the light version of the same colours
```

- The theme is the app's: every part of the kit is drawn with it from then on.
- Nothing small: body 16 pixels, code and secondary text 15, labels, badges and details 14.
- Amber means only "waiting for the user" (a pending approval or question, `needs you`, a nearly full context). Purple means running, and the selected session.
- Every control is drawn by the kit in the theme's colours, menus and text fields included: GPUI has no system controls.
- `minitel(dark)` returns a value like any other: change a colour or a size on it before setting it.

It is a member of the kit's workspace: `cargo test -p archipel-gpui-minitel` from `ui/gpui` checks what the theme promises. `cargo run -p archipel-gpui-catalog -- --theme minitel` shows every component with it, and `cargo run -p archipel-gpui-live --example chat -- --theme minitel` a real session.
