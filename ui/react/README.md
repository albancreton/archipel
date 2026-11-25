# React component kit

`@harness-bridge/react`: one component per kind of item, and a chat view assembled from them. Optional: an app can use the client SDK alone and draw everything itself.

```tsx
import { ChatView, useSession } from "@harness-bridge/react";
import "@harness-bridge/react/styles.css";

function Chat({ session, onFork }) {
  // session: a Session of @harness-bridge/client. onFork receives the copy to show instead.
  const { conversation, actions, openElsewhere, error } = useSession(session, { onFork });
  return <ChatView conversation={conversation} actions={actions} openElsewhere={openElsewhere} error={error} />;
}
```

- When another process of the harness has the session open (its own terminal UI, say), the daemon refuses messages for it. Given `openElsewhere`, the chat says so above the composer; given `onFork` too, it offers "Fork and continue here" and hands your app the copy. A refused message goes back into the field.

## Settings bar

`SettingsBar` draws the agent's settings (model, effort, permission mode, switches) from what the agent reports: a menu per choice, a checkbox per switch. It knows no model or mode by name. `ChatView` draws it above the composer when `actions.setOption` is there, which `useSession` provides.

```tsx
// In a session: what ChatView does for you.
<SettingsBar settings={settingsOf(state.values.get("configOptions"))} onChange={(id, value) => session.setConfigOption(id, value)} />

// Before a session exists: choose first, then create the session with the choices.
const { settings, onChange, initial } = useDraftSettings(options.configOptions); // options: await client.sessionOptions({ cwd, agentId })
<SettingsBar settings={settings} onChange={onChange} />
await client.newSession({ cwd, agentId, configOptions: initial });
```

- A change the agent refuses is shown under the bar, and the control goes back to the value in force.
- Settings before a session need an agent with `session.options` in its capabilities (`client.agents()`). Claude has it. Pi does not: `sessionOptions` fails, and Pi ignores `configOptions` on a new session, so on Pi create the session first and change settings in it (checked live, `session.initial_settings`).

## Session list

```tsx
<SessionList
  sessions={sessions}              // client.listSessions(), kept current with withSession / withoutSession
  agents={agents}                  // client.agents(): hides rename and delete where the agent cannot
  selected={session?.id}
  onOpen={(id) => client.session(id).then(show)}
  onRename={(id, title) => client.setTitle(id, title)}
  onDelete={(id) => client.deleteSession(id)}
  onMore={nextCursor && loadMore}  // with client.sessionsPage
/>
```

- Sessions are under their folder's name. A row says `needs you` (an approval or question is waiting), `running`, `open elsewhere`, and `from <agent>` for a session started outside the app.
- Delete asks first: the agent removes the session from its own history too, and that cannot be undone. The daemon wants the `admin` scope for it.
- `SessionRow`, `byFolder` and `ago` are exported for a list of your own.

## The rest

- Each component takes an item and, where the user can act, a callback. Nothing else.
- `components={{ "x.acme.checkpoint": Checkpoint }}` draws an extension's kind with your component. Without it the item gets a generic fallback.
- Styles are one CSS file with `hb-` classes and a few variables.
- Text is shown as written; Markdown is not rendered.

`pnpm test` draws all recorded sessions and checks that every message and tool call is on the page. `pnpm demo` writes them as HTML files to `demo/out`.
