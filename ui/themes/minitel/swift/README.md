# Minitel theme for the SwiftUI kit

`ArchipelMinitel`: the look recorded in [`../DESIGN.md`](../DESIGN.md), for the views of `ArchipelUI`. The same colours, sizes and rules as [`../react/styles.css`](../react/styles.css). `#282C34`, the terminal's own background, is the main one.

```swift
import ArchipelMinitel
import ArchipelUI

LiveChatView(session: session).archipelTheme(.minitel())      // dark, as in the app
SessionList(...).archipelTheme(.minitel(.light))              // the light version of the same colours
```

- `.archipelTheme(_:)` goes on any view above the kit's views; everything below takes the theme. It also sets the colour scheme, so the system's own menus and switches match.
- Nothing small: body 16 points, code and secondary text 15, labels, badges and details 14.
- Amber means only "waiting for the user" (a pending approval or question, `needs you`, a nearly full context). Purple means running.
- Buttons and text fields are drawn by the theme (the primary one in periwinkle with dark ink). Menus and switches are the system's, in the theme's colour scheme.
- `ArchipelTheme.minitel()` is a value like any other: change a colour or a size on it before passing it.

`swift test --filter ArchipelMinitelTests` (from the repository's root) draws recorded sessions, the requests waiting on the user, and the parts around a chat with the theme, dark and light, into `.build/archipel-ui-renders/minitel`. Text fields show as a striped box there: an off-screen picture cannot draw them. `swift run ArchipelCatalog --theme minitel` writes every view in each of its states to `ui/swift/Catalog/out/catalog-minitel.html` (real controls, dark and light), and `swift run ArchipelCatalog --live` shows them in a window. `swift run ArchipelChatDemo --theme minitel` shows a real session.
