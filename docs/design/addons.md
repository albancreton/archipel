# Addons

_2026-11-23. Asked by the user: keep the daemon's core a clean interface to the adapters, and put every fix for an upstream fault and every extra feature in a piece that can be removed by "remove an addon and recompile". The same mechanism is for consumers of the library, so that they can add a feature or a hotfix without forking._

## What an addon is

An addon is a piece of code inside the daemon that sits on both of its sides: between the daemon and an adapter, and between an app and the daemon. It sees the messages that pass and may change or stop them. The core knows nothing about what any addon does.

There are two kinds:

| Kind | What it is | By default | Life |
|---|---|---|---|
| **fix** | Works around something a harness, an adapter or the protocol gets wrong or leaves out | on | Removed when upstream has caught up |
| **feature** | Adds something no upstream offers | off | May stay for good |

What is **not** an addon: a bug of our own. That is fixed in the core, because no upstream release will ever make the fix unnecessary. Example from the same day: `session/delete` left the session in the daemon's list. The daemon now handles it.

## How it differs from an extension

The daemon already had extensions (`daemon.md`, section 11). The two do different jobs:

| | Extension | Addon |
|---|---|---|
| Runs | In its own process, any language | Inside the daemon, Rust |
| Connects | Like an app, with the `extend` scope | Compiled in |
| Can | Add its own items to a session and serve its own methods | The same, and see, change, stop, hold and answer every message between an app and an adapter, and send any of them itself |
| Cannot | Touch what the harness and the apps say to each other | Run outside the daemon or be written in another language |
| If it crashes | The daemon goes on | The daemon crashes |

## The interface

An addon is a Rust type that implements the trait `Addon` from the crate `bridge-addon-api`. That crate depends only on `serde_json`, so an addon crate does not pull in the daemon.

An addon can do three things: react to every message that passes, send messages of its own in either direction, and be called back later.

### Reacting: every message, both directions

| Message | Hook | The addon may |
|---|---|---|
| A request to the adapter: an app's, the daemon's own (`initialize`, the `session/resume` that wakes a session, `session/list`), or another addon's | `request` | Change the params; refuse it; answer it itself; hold it |
| The answer to such a request | `response` | Change it, also from an error to a result or back; hold it. It gets the request's method and params, and the same number as `request` did |
| A notification to the adapter, such as `session/cancel` | `notification` | Change it; drop it; hold it |
| A `session/update` from the adapter | `update` | Change it; drop it; hold it, before it is logged and passed to apps |
| Any other notification from the adapter, such as `auth/status_update` | `agent_notification` | Change it; drop it; hold it |
| A request made of the user: an approval, a question | `agent_request` | Change it; refuse it; answer it in the user's place; hold it |
| The user's answer to such a request | `answer` | Change it; hold it |
| An app's call of a method under `_x/` | `method` | Answer it; refuse it; hold it; pass, which leaves it to the next addon and then to the extension of that namespace |

Every message carries a number (`id`) and who it is from (`from`: an app with its name, the daemon, the adapter, or another addon by its id).

**Holding** works the same for all of them. The hook says "hold" and remembers the message's number. From any later hook the addon ends the hold:

| Call | A request, an approval, a method call | A response, an answer | A notification, an update |
|---|---|---|---|
| `cx.release(id, Pass)` | Goes on as it is | Goes on as it is | Goes on as it is |
| `cx.release_with(id, content)` | Goes on with these params | Goes on with this result | Goes on with these params, or this update body |
| `cx.release(id, Answer / Refuse)` | Ends here with that outcome | Goes on with that outcome | Dropped |
| `cx.discard(id)` | Ends as an error, since somebody waits | Goes on as an error | Dropped |

Nothing else waits behind a held message. A held approval is neither logged nor shown to apps until it goes on. A held update is logged when it is released, so after the ones that overtook it.

### The app side

The hooks above sit between the daemon and an adapter. A second set sits between an app and the daemon. It sees what never reaches an adapter or does not come from one: attaching to a session and its replay, the session list, the daemon's answer to an app's `initialize`, and every message on its way to one particular app.

| Message | Hook | The addon may |
|---|---|---|
| Any request from an app, before the daemon handles it | `app_request` | Change the params; refuse it; answer it itself; hold it |
| The answer to it, on its way to that app: the daemon's own, or an adapter's as the other hooks left it | `app_response` | Change it; hold it |
| A notification from an app, before the daemon handles it | `app_notification` | Change it; drop it; hold it |
| An app's answer to a request it was sent | `app_answer` | Change it; drop it, which leaves the request open for another answer; hold it |
| A notification or a request on its way to one app: a session update (live or replayed), a pending approval, the daemon's own notifications, an addon's | `to_app` | Change it; drop it for that app; hold it |

Each of these hooks is told which app it is about: its connection number, the name it gave, and its scope. So what an app is told can depend on the app.

The two sides are passed one after the other. An app's `session/prompt` passes `app_request` of every addon, then the daemon, then `request` of every addon, then reaches the adapter. The answer comes back through `response` of every addon, the daemon, then `app_response`.

Three things differ from the adapter side:

- **`applies` is not asked.** These hooks are called for every app and every agent. The hook is given the agent of the session the message names, when it names one.
- **A change made in `to_app` is not stored.** The log holds the update as the adapter-side hooks left it; `to_app` changes what one app receives, each time it is sent, including on replay. To change what is stored, use `update`.
- **Everything sent to an app passes `to_app` of every addon**, also what an addon added or sent itself.

### Acting: standing in for either side

Through the context every hook receives (`Cx`), an addon sends what an app or an adapter could send:

| As | Call | What happens |
|---|---|---|
| The app | `cx.call(method, params)` | A request to the adapter. The answer arrives in the hook `reply`. If the adapter stops first, the reply is an error |
| The app | `cx.notify(method, params)` | A notification to the adapter, such as `session/cancel` |
| The adapter | `cx.append(session, update)` | An update added to a session: logged, replayed and sent to apps like the adapter's, marked `_meta.bridge = {origin: "addon", addon: <id>}` |
| The adapter | `cx.announce(method, params)` | A notification to apps that is not a session update. Not logged. With a `sessionId` it goes to the apps attached to that session, otherwise to every app |
| The adapter | `cx.ask(method, params)` | A request for the user to answer, handled exactly like an adapter's approval: logged in the session its `sessionId` names, marked as the addon's, shown to every attached app, first answer wins. The answer arrives in `reply`. If the session's adapter stops first, the reply is a cancellation |

| An app, towards the daemon | `cx.app_call(method, params)` | A request the daemon handles as if an app had made it: `session/new`, `session/prompt`, `_bridge/sessions/list`. Unlike `cx.call`, the daemon does its part: it records a new session, wakes one that is not live. The answer arrives in `reply`. It does not pass the app-side hooks; what the daemon sends an adapter for it passes every addon's `request`, marked as from this addon |
| The daemon, towards one app | `cx.tell(app, method, params)` | A notification to that app alone. Not logged |

In the other direction, an app reaches an addon with a call of a method under `_x/` (hook `method`) or a notification under `_x/` (hook `notification`). Neither ever reaches an adapter.

### Being called back, and reading

| Call or hook | What it is for |
|---|---|
| `cx.wake(after)`, then hook `timer` | To be called after a time. This is how an addon puts a time limit on something it holds, or acts on a turn that runs too long |
| Hook `stopped` | An adapter stopped. By then every request that waited on it has failed, the addon's own included |
| `cx.last_update(session, kind)`, `cx.updates(session)` | Read a session's log |
| `cx.session(session)` | The session's agent, folder, title and turn state |

Two more methods describe the addon: `info()` (its id, kind, what it does, what fault it stands in for, what has to happen upstream for it to go, which conformance scenario checks that) and `applies(agent)` (limits it by harness, adapter name and version, advertised capabilities, operating system). Every hook has a default that does nothing.

### Several addons are layers

The first addon added is nearest the app, the last nearest the adapter. A message towards the adapter passes them first to last; a message towards the app last to first.

- An addon that answers a request itself is the adapter as far as the layers nearer the app can tell: they see the response, the layers nearer the adapter see nothing. The same holds the other way round for a request of the adapter answered in the user's place.
- What an addon sends itself starts at its own layer and passes only the layers beyond it, in the direction it travels. Its request and its notification to the adapter pass the layers nearer the adapter. Its update, its notification to apps and its request of the user pass the layers nearer the app, which may change, answer or hold them like an adapter's. The answer comes back through the same layers and stops at the addon that asked. An addon is never shown its own messages.
- A released message goes on from the layer that held it.

### Rules an addon lives by

- The daemon handles one message at a time on one task and calls the hooks from there. A hook returns quickly and does not block. An addon that has to wait holds the message and releases it from a later hook.
- What an addon asks for through `Cx` happens when its hook returns, before the message it was looking at moves on. So a request it sends from the `request` hook reaches the adapter before the request it was looking at.
- The daemon puts no time limit on a hold. A held message that is never released stays held, unless its adapter stops: then held requests and responses fail, a held answer is recorded as cancelled, and the rest is dropped. An addon that wants a limit sets a timer.
- An addon keeps no state that must survive a restart. What it needs to know about a session it reads from the log.
- What an addon sends goes to the adapter of the message it is looking at. For a method call or a notification under `_x/` that is the adapter of the session the params name, or the first agent.
- Ids are `<scope>.<name>`: `acp.` for any adapter, `claude.`, `pi.` for one harness, anything else for a consumer's own.
- While `initialize` itself passes the hooks, the adapter's name, version and capabilities are not known yet, so an addon that limits itself by those does not see it.

### Where the interface ends

Three things are outside it:

- **The adapter's other notifications are not passed to apps by the daemon**, with or without addons. Some carry private data (Claude's `auth/status_update` has the account's email address). An addon that wants apps to have one sends its own with `cx.announce`.
- **A request of the adapter that an addon answers is still sent to the attached apps**, followed at once by its resolution, because that is how the log records it.
- **An addon acts on one adapter at a time**, the one of the message at hand. It cannot address another agent's adapter from the same hook.

### How it is tested

`crates/bridge-server/tests/addons.rs` (the adapter side) and `tests/app_side.rs` (the app side) have one test per capability and are the reference for exact behaviour. The adapter in them is scripted by the test, so each check reads what the adapter and the app actually received.

Two scripts say whether those tests are enough:

| Script | What it answers | Fails when |
|---|---|---|
| `scripts/addon-coverage.sh` | Which lines of the addon system does no test run? | A line is neither run nor marked `cov:` with the reason no test reaches it |
| `scripts/addon-mutations.py` | Would a test fail if a rule were broken? It breaks one rule at a time in the daemon (layer order, who may release a hold, who sees whose messages, …) | A broken rule goes unnoticed |

The second exists because the first is not enough: with every line run, two of twelve broken rules were still not noticed. A new rule of the addon system gets a line in the mutation list.

## Where addons come from

| Source | How it gets into a daemon |
|---|---|
| The library: `crates/bridge-addons`, one file per addon under `acp/`, `claude/`, `pi/` | `bridge_addons::all()`, which `bridged` uses |
| A consumer's own crate | Their own daemon binary: `bridged::main(addons)` is the whole `bridged` program with a list of addons, so the binary is five lines. `examples/own-daemon` |
| A consumer who embeds the daemon | `Server::builder().addon(…)` |

Rust has no stable binary interface, so an addon cannot be dropped in as a file next to a released `bridged`: it is compiled in. Two ways around that exist if they are ever wanted: addons as WebAssembly modules, or the hooks offered over a connection the way extensions are. Neither is built.

Which addons run is decided in `config.json`:

```json
{"addons": {"acp.restore-settings": false, "example.no-bypass": true}}
```

An addon that is not named runs if it is a fix and does not if it is a feature. A name the daemon does not have is an error at start. `bridged addons` prints every addon compiled in, whether it runs, and its `info`.

## How a fix gets removed

Every fix names a conformance scenario (`conformance/`). The scenario records two things: what happens with the addons as shipped, and what the harness does with the addon switched off. When an upstream release repairs the fault, the second one shows `CHANGED` on the next run. That is the signal: delete the addon's file and its line in `bridge_addons::all()`, and drop the "unaided" part of the scenario.

A platform-specific fault is handled inside the addon: `applies` gets the operating system, and Rust's `#[cfg(target_os = …)]` is there for code that only compiles on one.

## The addons the library ships

| Id | Kind | For | What it does | Until |
|---|---|---|---|---|
| `acp.restore-settings` | fix | Any adapter that advertises `session.options` (Claude's does) | Adds the session's last logged settings to the `session/resume` that wakes it, model first | The `configOptions` field on `session/resume` is stable in ACP, and then this moves into the core; or adapters restore settings themselves |
| `pi.check-setting-values` | fix | Pi | Refuses a value that a setting does not offer, with error -32602 | `pi-acp` refuses it |

Both were checked against the real harnesses, on and off: `docs/research/24-addons.md`.
