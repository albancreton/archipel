# React component kit

`@harness-bridge/react`: one component per kind of item, and a chat view assembled from them. Optional: an app can use the client SDK alone and draw everything itself.

```tsx
import { ChatView, useSession } from "@harness-bridge/react";
import "@harness-bridge/react/styles.css";

function Chat({ session, onFork }) {
  // session: a Session of @harness-bridge/client. onFork receives the copy to show instead.
  const { conversation, actions, openElsewhere } = useSession(session, { onFork });
  return <ChatView conversation={conversation} actions={actions} openElsewhere={openElsewhere} />;
}
```

- When another process of the harness has the session open (its own terminal UI, say), the daemon refuses messages for it. Given `openElsewhere`, the chat says so above the composer; given `onFork` too, it offers "Fork and continue here" and hands your app the copy. A refused message goes back into the field.

- Each component takes an item and, where the user can act, a callback. Nothing else.
- `components={{ "x.acme.checkpoint": Checkpoint }}` draws an extension's kind with your component. Without it the item gets a generic fallback.
- Styles are one CSS file with `hb-` classes and a few variables.
- Text is shown as written; Markdown is not rendered.

`pnpm test` draws all recorded sessions and checks that every message and tool call is on the page. `pnpm demo` writes them as HTML files to `demo/out`.
