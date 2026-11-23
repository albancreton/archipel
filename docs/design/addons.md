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
| Can | Add its own items to a session and serve its own methods | See, change and stop the messages between the daemon and an adapter |
| Cannot | Touch what the harness and the apps say to each other | Add items or methods of its own (not yet, see below) |
| If it crashes | The daemon goes on | The daemon crashes |

## The interface

An addon is a Rust type that implements the trait `Addon` from the crate `bridge-addon-api`. That crate depends only on `serde_json`, so an addon crate does not pull in the daemon.

| Method | Called | The addon may |
|---|---|---|
| `info()` | When listing | Say its id, kind, what it does, what fault it stands in for, what has to happen upstream for it to go, and which conformance scenario checks that |
| `applies(agent)` | For every message | Limit itself by harness (`claude`, `pi`), adapter name and version, advertised capabilities, operating system |
| `request(request)` | Before the daemon sends a request to an adapter: an app's, or its own such as the `session/resume` that wakes a session | Change the params, or refuse (the asker gets a JSON-RPC error marked with the addon's id). It can read the session's log |
| `update(update)` | When an adapter sends a `session/update`, before it is logged and passed to apps | Change it, or drop it |

Every method except `info` has a default that does nothing. Addons see a message in the order they were added.

Rules an addon lives by:

- The daemon handles one message at a time on one task and calls the hooks from there. A hook returns quickly and does not block.
- An addon keeps no state that must survive a restart. What it needs to know about a session it reads from the log.
- Ids are `<scope>.<name>`: `acp.` for any adapter, `claude.`, `pi.` for one harness, anything else for a consumer's own.

These four methods are what the first addons needed. More hooks are added when an addon needs them, not before. Known candidates: the adapter's answer to a request, an addon's own requests to the adapter, items and methods of its own (what an extension can do today from outside).

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
