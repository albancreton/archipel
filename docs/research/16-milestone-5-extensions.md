# Milestone 5: extensions

_2026-11-22. Checked by `cargo test --workspace` (41 tests), including a toy extension run as its own process._

## The requirement

A product such as T3 Code must be able to add its own content (checkpoints, handoffs) without changing or forking the server.

## What an extension is

A separate program that connects to the daemon like an app, with the `extend` scope, and says which namespace it is:

```
_archipel/extension/register {"namespace": "demo"}
```

From then on:

| It can | How |
|---|---|
| See what happens | Like any app: `_archipel/sessions/updated` arrives for every session; it can attach to any of them |
| Add items to a session | `_archipel/session/append` with an update whose kind starts with `x.demo.` |
| Serve methods | The daemon passes it every request whose method starts with `_x/demo/` and returns its answer to the caller |

The daemon numbers, stores, replays and fans out the extension's updates exactly like the agent's. It never looks inside them. An extension can be written in any language, and one that crashes takes nothing else down: calls waiting on it fail, and its namespace is free again.

## Items that explain themselves

The one real design question was how a client that has never heard of an extension can hold its items correctly: where in the timeline, and whether a second update is a new item or a change to an existing one.

ACP answers this for its own messages with state rules in the schema. An extension's update carries the same kind of rule with it:

```
_archipel/session/append {
  "sessionId": "…",
  "update": {"sessionUpdate": "x.demo.checkpoint", "checkpointId": "c1", "label": "after turn 1"},
  "state":  {"action": "upsert", "id": "checkpointId"}
}
```

The daemon writes the rule into the stored update (`_meta.archipel.state`). Every client's reducer applies it like an ACP rule, with the update's kind as the target. So:

- the item gets a place in the timeline when it first appears,
- a later update with the same `checkpointId` changes that item,
- this works in a replay, when the extension is not running, and in a client that knows nothing about it.

Because the rule travels with the update, there is no registry of extension schemas to keep in step between extension, daemon and clients.

A UI that has no component for `x.demo.checkpoint` skips the item or shows a generic fallback (the text rendering prints the kind and the JSON). An update without a rule is kept as an event and not placed among the items.

## The toy extension

`archipel-demo-checkpoints` (about 90 lines, `crates/archipel-client/src/bin/demo_checkpoints.rs`):

- adds an `x.demo.checkpoint` item whenever a session's turn ends,
- serves `_x/demo/checkpoints/list` and `_x/demo/checkpoint/restore`; restoring marks the item, which shows as a change to the same item.

It touches no files. A real one would record a git ref and reset to it, and the daemon would not change.

A test starts it as a process next to a daemon and the command-line client: after the recorded turn, `archipel show` prints the session as recorded plus one checkpoint line, `archipel call _x/demo/checkpoints/list` returns it, and after `restore` the same item reads `"restored":true`.

## Rules the daemon enforces

- Registering and appending need the `extend` scope (`archipeld token add NAME --scope extend`). The local socket has every right.
- An extension appends only kinds in its own namespace, never an ACP kind or another extension's.
- One connection per namespace.
- Calling an extension's method needs `interact`, like any request that is not one of the six reading ones.

## Not in this version

- **Extensions that block or change the flow**, for example approving a prompt before it reaches the agent. That needs ordering and timeout rules. The namespaces leave room for it.
- **Reading methods for observers.** An observer cannot call `_x/…` methods, even ones that only read.
- **Telling apps which extensions are present** as it changes. `initialize` lists the namespaces registered at that moment.
- **In-process extensions.** A Rust product can already build its own server from the crates; no plugin interface was added for it.
- **`_x/` methods of an adapter** are no longer passed to the adapter: the prefix now belongs to extensions. Other `_`-prefixed methods still pass.
