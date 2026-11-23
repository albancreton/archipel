# schema

The data model.

| File | What | Source |
|---|---|---|
| `acp-v2.schema.json` | The pinned ACP v2 schema (preview, including unstable parts) | Copied from `@agentclientprotocol/sdk` 2.9.1, `schema/v2/schema.unstable.json`. Do not edit. |
| `state-rules.json` | How each ACP message changes client state: append, upsert, replace or remove, on which target, keyed by which id | Generated from the `x-state` annotations in `acp-v2.schema.json` |
| `bridge.schema.json` | The daemon's extensions: `_bridge/` methods and `_meta.bridge` | Generated from the Rust types in `crates/bridge-schema/src/ext.rs` |

Run `just gen` after changing the pinned schema or the extension types. CI fails if the generated files are stale.

Moving to a newer ACP v2 revision is one deliberate change: replace `acp-v2.schema.json`, bump the `agent-client-protocol-schema` crate to the matching release, run `just gen`, re-record or re-check the transcripts in `testdata/`.

The Rust types for ACP come from the official `agent-client-protocol-schema` crate, which is the upstream source of this schema. A test parses every recorded update with those types to catch a mismatch between the crate and the pinned file.
