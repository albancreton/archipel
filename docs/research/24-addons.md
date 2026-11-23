# Addons: the mechanism, the first two, and the fixes that went into the core

_2026-11-23. Follows `23-conformance-first-run.md`, which found five problems. The user did not want fixes for upstream faults written into the core, and asked for an addon mechanism that consumers of the library can use too. The design is in `docs/design/addons.md`; this note records what was decided, what was found on the way, and what was checked._

## What the user asked for

1. Fixes for adapter or protocol faults are isolated, so that removing one is "remove an addon and recompile".
2. Consumers can add a feature or a hotfix the same way, without forking and without waiting for a release.
3. The library ships recommended addons (fixes) and optional ones (features).
4. The core stays a clean interface to the adapters.
5. Addons can be about one harness (Claude, Pi, later Codex), and may have to deal with a platform.

## Where each of the five problems went

| Problem from note 23 | Where it was fixed | Why there |
|---|---|---|
| 1, 2, 3: a reopened Claude session loses its model and effort, and the app shows the old choice until then | Addon `acp.restore-settings` | See the finding below |
| 4: a delete through the daemon leaves the session in the daemon's list | Core | Our own bug. No upstream release would ever remove the need for the fix |
| 5: Pi accepts an effort value it does not offer | Addon `pi.check-setting-values` | `pi-acp` accepts anything. I first counted this as ours because the translator in front of `pi-acp` is ours, but the translator's job is to change shapes, and the missing check is upstream's |

## A finding that changed problem 1

Note 23 put the lost model down to the adapter's order of preference (environment variable, then `settings.json`, then the session's transcript). That is what the code does, but it is not the whole picture. ACP v2 has a field for exactly this case: `configOptions` on `session/new`, `session/resume` and `session/fork`, a list of settings the agent applies in order before anything else, skipping any it can no longer apply. The schema marks it unstable and says a client must only send it to an agent that advertises `session.options`. Claude's adapter does.

So by the protocol's design, sending a session's choices again on resume is the client's job, as it already is for MCP servers. The daemon is that client and was not doing it. This is less "the adapter is wrong" than "the protocol leaves it to us".

It is still an addon, for one reason: the field is unstable. If ACP changes or drops it, the addon is switched off or rewritten without touching the core. When it becomes stable, the right move is to fold it into the core next to the MCP servers, and the addon's `until` says so.

It also made the mechanism smaller. My first design re-applied the settings with one `session/set_config_option` call per setting after the resume, which needed addons to make their own calls, receive the answers, and hold back the app's waiting requests meanwhile. With the field, one hook that edits the outgoing `session/resume` is enough, and the adapter does the ordering.

## What was built

- `crates/bridge-addon-api`: the trait and the types its hooks receive.
- `crates/bridge-server`: `Builder::addon` and `addons`; the daemon calls the hooks at the two points where every message to and from an adapter passes.
- `crates/bridge-addons`: the two addons, `all()` and `recommended()`.
- `crates/bridged`: now a library with a five-line program on top; `addons` in `config.json`; the command `bridged addons`.
- `examples/own-daemon`: a daemon with an addon of its own (a house rule that refuses the mode that skips approvals).
- Core: `session/delete` is handled by the daemon. Once the harness agrees, the session leaves the log and every app receives the new notification `_bridge/sessions/removed`. A session open in another process of the harness is not deleted, by the same rule as any other action on it. The required scope is still `interact`; whether it should be `admin` is an open decision.
- Agents now say which harness they are (`AgentSpec.harness`), for addons that are about one.

## What was checked

Automated, without a harness:

- The hooks, with a scripted adapter: a request changed, a request refused (the adapter never receives it, the app gets the marked error), an update changed and another dropped (in what apps receive and in what a later replay shows), the daemon's own `session/resume` passing the addons with the session's log to read, and an addon left out for an agent it does not apply to.
- Each addon alone: what it adds or refuses, and what it leaves untouched.
- Deleting: refused by the harness (nothing changes), accepted (gone from the log, the other app is told), and then unknown.

Live, with the conformance scenarios, each with the addon on and off:

| Scenario | Harness | Addon on | Addon off |
|---|---|---|---|
| `reopen.keeps_settings` | Claude (Haiku) | Model kept, and the next turn really ran on it; effort kept; mode kept | Identical to the baseline of note 23: model and effort lost |
| `reopen.keeps_settings` | Pi | Unchanged (Pi does not advertise `session.options`, so the addon does not apply) | Same |
| `settings.rejects_unknown` | Pi | Error -32602 | Accepted, as in note 23 |
| `settings.rejects_unknown` | Claude | Error -32602 | Same (the addon is for Pi only) |
| `session.rename_delete` | Claude | The session is gone from the daemon's list | (core, no switch) |

The "off" results are now part of the baselines (`reopen.keeps_settings.unaided`, and the fact `unknownEffortUnaided`). A harness release that repairs either fault will show there as `CHANGED`.

Cost: 12 short Haiku turns for Claude, and Pi on the local model.

## Not done, and limits

- **An addon cannot yet see the adapter's answers, make requests of its own, or add items and methods.** Nothing needed them. A feature addon such as rewind will, and they are added then. _(Added the same evening at the user's request: see the section at the end.)_
- **Addons are compiled in.** Somebody who uses the released `bridged` binary cannot add one without building their own daemon. `examples/own-daemon` shows that this is five lines, but it is a build.
- **Problem 3 is solved only as far as the settings can be restored.** If a model is gone when the session wakes, the adapter skips it and the display changes at that moment, as before. The app is not told why.
- **The plan-mode behaviour seen once in note 23** (Haiku selected, turns on Sonnet) is untouched and still has no scenario.
- **The SDKs do not act on `_bridge/sessions/removed` yet**, as they do not on `_bridge/sessions/updated`. Both have the name as a constant only.

## Later the same day: the full set of capabilities

The user asked to "fully feature and test the addon capabilities, allowing to see and manipulate requests and responses". The first version had two hooks. It now covers every message in both directions; the table is in `docs/design/addons.md`.

What was added:

- Hooks for the answer to a request (`response`), an app's notification (`notification`), a request the adapter makes of the user (`agent_request`) and the user's answer to it (`answer`).
- A request can be answered by an addon, not only refused, and it can be held and released later.
- An addon can send requests of its own to the adapter and gets the answers (`reply`), can add updates to a session, and can serve methods under `_x/`.
- `initialize` now passes the hooks like every other request, so an addon can change what the daemon declares to an adapter and what the adapter's capabilities look like to apps.
- Every request has a number that the `request` and `response` hooks both receive, so an addon can pair them.

Decisions taken on the way:

- **Addons are layers.** With more than one addon the order has to mean something. The first added is nearest the app. A message passes the layers in one order going out and the reverse coming back, and an addon that answers a request is the adapter for the layers nearer the app. This is the usual middleware model; anything else made the "who sees what" question unanswerable.
- **An addon's own messages start at its layer.** Its requests pass only the layers nearer the adapter, its updates only those nearer the app, and it never sees them itself. Otherwise an addon that reacts to a request by sending one would trigger itself.
- **Actions happen when the hook returns, before the message moves on.** A hook cannot wait for anything, because the daemon handles one message at a time. So an addon asks for things and the daemon carries them out right after. Waiting for an answer is done by holding the request.
- **A hold has no timeout.** The addon that holds is responsible for releasing. If the adapter stops, held requests fail like any open request.
- **A request of the adapter answered by an addon stays in the history**, as the request and its resolution with `resolvedBy: "addon <id>"`. An approval given by a rule should be as visible afterwards as one given by a person.
- **The log is read through the context**, by session id, not handed over with the request. Hooks that have no session at hand can still read one.

Checked: twelve tests with a scripted adapter, one per capability and one for the layering with two addons, each reading what the adapter and the app actually received. They were run eight times in a row without a failure. Then the live conformance scenarios again, because the path every message takes had changed: Claude's free scenarios and `reopen.keeps_settings` (six Haiku turns), and Pi's two, all `same`.

The four things still left out are listed in the design note.
