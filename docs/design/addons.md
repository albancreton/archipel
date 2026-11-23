# Addons

_2026-11-23. Asked by the user: keep the daemon's core a clean interface to the adapters, and put every fix for an upstream fault and every extra feature in a piece that can be removed by "remove an addon and recompile". The same mechanism is for consumers of the library, so that they can add a feature or a hotfix without forking._

## What an addon is

An addon is a piece of code that sits between the daemon and an adapter. It sees the messages that pass and may change or stop them. The core knows nothing about what any addon does.

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
| Can | Add its own items to a session and serve its own methods | The same, and see, change, stop and answer the messages between an app and an adapter |
| Cannot | Touch what the harness and the apps say to each other | Run outside the daemon or be written in another language |
| If it crashes | The daemon goes on | The daemon crashes |

## The interface

An addon is a Rust type that implements the trait `Addon` from the crate `bridge-addon-api`. That crate depends only on `serde_json`, so an addon crate does not pull in the daemon.

An addon sees every message between an app and an adapter, in both directions:

| Message | Hook | The addon may |
|---|---|---|
| A request to the adapter: an app's, or the daemon's own (`initialize`, the `session/resume` that wakes a session, `session/list`) | `request` | Change the params; refuse it; answer it itself; hold it |
| The answer to such a request | `response` | Change it, also from an error to a result or back. It gets the request's method and params, and the same request number as `request` did |
| A notification from an app, such as `session/cancel` | `notification` | Change it or drop it |
| A `session/update` from the adapter | `update` | Change it or drop it, before it is logged and passed to apps |
| A request the adapter makes of the user: an approval, a question | `agent_request` | Change it; refuse it; answer it in the user's place |
| The user's answer to such a request | `answer` | Change it |

Besides reacting, an addon can act. It does so through the context every hook receives (`Cx`):

| Action | What happens |
|---|---|
| `cx.call(method, params)` | Sends a request of the addon's own to the adapter. The answer arrives in the hook `reply`. If the adapter stops first, the reply is an error |
| `cx.append(session, update)` | Adds an update to a session. It is logged, replayed and sent to apps like the adapter's, marked `_meta.bridge = {origin: "addon", addon: <id>}` |
| `Verdict::Hold`, then `cx.release(…)` or `cx.release_with(params)` | Keeps a request until the addon lets it go on, changes it, refuses it or answers it, from any later hook. Other requests are not kept waiting |
| `cx.last_update(session, kind)` | Reads the most recent update of a kind from the session's log |

And it can serve methods of its own: an app's call to a method under `_x/` is offered to the addons in order (hook `method`) before it goes to an extension. An addon can hold such a call too, for example to ask the adapter something first.

Two more methods describe the addon: `info()` (its id, kind, what it does, what fault it stands in for, what has to happen upstream for it to go, which conformance scenario checks that) and `applies(agent)` (limits it by harness, adapter name and version, advertised capabilities, operating system). Every hook has a default that does nothing.

### Several addons are layers

The first addon added is nearest the app, the last nearest the adapter. A message towards the adapter passes them first to last; a message towards the app last to first.

- An addon that answers a request itself is the adapter as far as the layers nearer the app can tell: they see the response, the layers nearer the adapter see nothing. The same holds the other way round for a request of the adapter answered in the user's place.
- What an addon sends itself starts at its own layer. Its request passes the layers nearer the adapter, there and back, and its update passes the layers nearer the app. It is never shown its own messages.

### Rules an addon lives by

- The daemon handles one message at a time on one task and calls the hooks from there. A hook returns quickly and does not block. An addon that has to wait holds the request and releases it from a later hook.
- What an addon asks for through `Cx` happens when its hook returns, before the message it was looking at moves on. So a request it sends from the `request` hook reaches the adapter before the request it was looking at.
- A held request that is never released is never answered, unless its adapter stops. There is no timeout.
- An addon keeps no state that must survive a restart. What it needs to know about a session it reads from the log.
- Ids are `<scope>.<name>`: `acp.` for any adapter, `claude.`, `pi.` for one harness, anything else for a consumer's own.
- While `initialize` itself passes the hooks, the adapter's name, version and capabilities are not known yet, so an addon that limits itself by those does not see it.

What is still left out:

- A request of the adapter cannot be held, only passed, changed, refused or answered.
- A request of the adapter that an addon answers is still sent to the attached apps, followed at once by its resolution, because that is how the log records it. An app sees it appear and resolve.
- Notifications from an adapter other than `session/update` are not shown to addons. The daemon drops them, except the login state.
- An addon cannot send a request to an app, or a notification to an adapter.

`crates/bridge-server/tests/addons.rs` has one test per capability and is the reference for exact behaviour.

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
