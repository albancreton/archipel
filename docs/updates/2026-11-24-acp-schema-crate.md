# acp-schema-crate: 1.21.2 to 1.22.0

## In short

- The crate holds the Rust types for the ACP schema we pin. Nearly all of the diff is doc-comment rewording ("plain text", "may contain Markdown"). The v1 deserializer tightening does not reach us: `archipel-schema` re-exports only `v2`.
- One change is structural. The v2 `MessageMcpRequest` (`mcp/message`, unstable) gains optional `sessionId` and `toolCallId`. Nothing in our code builds or matches that type, so the bump should compile and its tests should pass unchanged. I have not built it.
- It matters because the daemon routes an adapter's incoming request by `params.sessionId`. Today an `mcp/message` with no session it knows is refused with `-32601`. Once an adapter sends `sessionId`, the request would be logged as open in that session and shown to apps as an unknown method. Nothing we record sends this yet, so the risk arrives with an adapter upgrade.
- The update is small in code, but it must not land alone. The crate and the pinned schema (still the 2.9.1 text) must move together, and that schema move is the subject of `docs/updates/2026-11-24-acp-sdk.md`. Decide the routing question first.

## What changed upstream

**MCP over ACP (v2, unstable), the only structural change.**
- `MessageMcpRequest` gains `session_id` (`sessionId`, optional): "the session whose work caused this request". The Agent "SHOULD include" it when it knows the session and "MUST NOT" send another session's ID.
- It gains `tool_call_id` (`toolCallId`, optional). The Agent "MUST NOT send it without `sessionId`".
- `x-scope` gains `"session": ["sessionId"]` and `"toolCall": ["toolCallId"]`. A schemars `if`/`then` makes `sessionId` required when `toolCallId` is present. That constraint exists only in the generated schema: the Rust deserializer does not check it. `tool_call_id` is `DefaultOnError`, so a malformed `toolCallId` becomes `None` silently, while a malformed `sessionId` is an error.

**v1 only (`src/v1/agent.rs`, `src/v1/mcp.rs`).**
- `SessionConfigOptionValue::ValueId` now rejects `type: "boolean"` with a non-boolean value. Before, it fell through to the id variant.
- `McpServer::Stdio` now rejects any `type` other than `stdio`.

**Wording only (v2, every other hunk).** Titles, names, labels, form fields, error messages and permission titles are now described as plain text. Several descriptions are now marked "May contain Markdown" (`SessionConfigOption`, `SessionConfigSelectOption`, `RequestPermissionRequest`, `SubagentUpdate`). `TextContent` now says "Clients SHOULD join adjacent text blocks without a separator before rendering". I read every v2 hunk outside `mcp.rs`. Each changes only doc comments, with no serde attribute or type change.

**Crate version.** `Cargo.toml` goes from 1.21.2 to 1.22.0. The features we use (`unstable`, `unstable_protocol_v2`) are unchanged.

No changelog was provided, so the crate's release cannot be tied to a schema version from the diff alone.

## What it touches here

- **`crates/archipel-schema/Cargo.toml` (line 10) and `Cargo.lock`.** The pin moves to 1.22.0 if we take the bump. The crate is the upstream source of the schema, so the two must match.
- **`schema/acp-v2.schema.json`, `MessageMcpRequest` (lines 2776–2818).** The pinned copy lacks `sessionId` and `toolCallId`, and its `x-scope` has only `mcpServer`. Its description is the "before" text in the diff, so the pin is the 2.9.1 text. A crate bump without a schema move leaves the Rust type and the file disagreeing. No test compares them: the only crate test covers `SessionUpdate`.
- **`schema/state-rules.json` and the generated copies.** The diff contains no `x-state` annotation, and the rule generator reads only `x-state` (`tools/watch/lib.ts` says so). A crate bump alone does not change any state rule.
- **Our consumers of the crate.** `crates/archipel-schema/src/lib.rs:11` re-exports `v2` as `acp`. The only use is `crates/archipel-core/tests/transcripts.rs:203`, which parses each recorded update as `acp::SessionUpdate`. Nothing constructs or matches `MessageMcpRequest`, and the v1 types are unreachable from our code.
- **Daemon routing (`crates/archipel-server/src/daemon.rs`).** I traced this path, which the sibling note left open. Any adapter request other than `session/update` becomes an `Ask` (around line 3097). `ask` runs the addon hooks, then `log_ask`, which reads the session from `text(&ask.params, "/sessionId")` (line 2990). If that session is unknown, the daemon answers `-32601` ("the request names no session the daemon knows"). If it is known, the request is recorded and inserted as pending, so apps see it. So an `mcp/message` with `sessionId` becomes an open request in that session with no handler behind it. `render.rs` `request_line` labels any other method generically. I read this path but did not run it.
- **Addons (`crates/archipel-addons`).** `acp.restore-settings` (`session/resume` `configOptions`) and `pi.check-setting-values` (`session/set_config_option`) are untouched by this diff. Upstream has not fixed either fault, so neither can go.
- **Profiles (`crates/archipel-agents`).** The Pi translator forwards `mcpServers` as plain JSON (`src/pi.rs` 324–328, 425), not through the crate, so the v1 change cannot reach it. The only MCP code in the crate is there. The Claude profile has no MCP handling.
- **Recordings (`testdata/transcripts`, `testdata/daemon`).** None contains `mcp/message`, and none declares a non-empty `mcpServers`. No re-recording is needed for this diff.
- **Conformance (`conformance/baseline`).** `session.mcp_server` (`conformance/scenarios.ts` 463–470) passes one stdio server. That is valid under both the old and new shapes, so the baseline is unaffected.
- **Client SDKs and kits.** No new item kind, method or capability appears in the diff, and no code under `clients/` or `ui/` mentions `mcp/message`. The text-joining guidance already matches: the TypeScript client joins neighbouring text blocks (`clients/ts/src/state.ts` 227–237), as does the Swift client (`clients/swift/Sources/ArchipelClient/State.swift` 192–206). The React kit joins with `""` (`ui/react/src/items.tsx` 24, 40), and the Swift kit with no separator (`ui/swift/Sources/ArchipelUI/Items.swift` 11, 82). Titles are drawn as text, not Markdown.
- **The sibling note `docs/updates/2026-11-24-acp-sdk.md`.** I checked its main claims against the code and the pin, and they hold. I disagree with its step 1 ("all pass unchanged") on one point: the routing path above is not inert once an adapter sends `sessionId`. Its statement that the daemon has no handler for `mcp/message` is correct, but the request is not dropped; it takes the path above.

## Proposed steps

1. **Pin current behaviour with a test, no product change.** In `crates/archipel-server/tests/daemon.rs`, have a fake adapter send an `mcp/message` request, first without `sessionId` and then with a known one. Assert today's outcome: `-32601` in the first case, a pending request in the second. Check: `cargo test -p archipel-server`.
2. **Decide what the daemon should do with `mcp/message`.** Options: refuse it explicitly whatever its `sessionId`, or support it properly, which is a larger job. The first is a fix of our own: the generic path turns an MCP call into a question for the user. Per `AGENTS.md`, a bug of this library is fixed in the core. An addon is for an upstream fault. This reading is mine; confirm it before coding.
3. **Move the crate and the pinned schema together, once step 2 is settled.** Bump `crates/archipel-schema/Cargo.toml` to 1.22.0 and replace `schema/acp-v2.schema.json` with the release that matches it (see below). Run `just gen`. Expect `schema/state-rules.json` and the generated copies to be unchanged. Check: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm -r check && pnpm -r test` after `cargo build`, and `swift test`.
4. **Add a drift test for `MessageMcpRequest`.** Compare the crate's schemars output for that type with the pinned `$defs` entry, so the two cannot disagree silently again. Check: the new test fails if the pinned file is reverted and passes after step 3.
5. **Before any adapter is trusted with ACP-transport MCP, probe it live.** Run a real Claude session on the cheapest model with one `{"type":"acp",...}` server, and record what reaches the daemon. If we decide to support the path, add a conformance scenario beside `session.mcp_server`.

No transcript, addon, profile or kit change is needed for this diff.

## What I could not judge

- **Whether 1.22.0 is the crate release matching the 2.12.0 schema.** The new `MessageMcpRequest` wording matches the sibling note's 2.12.0 description, but I had neither the 2.12.0 schema nor a changelog. Settle it by comparing the crate's description strings with the 2.12.0 `schema.unstable.json`.
- **Whether the bumped crate compiles and its tests pass.** I did not build, because that would write into `target/`. Settle it with `cargo test -p archipel-schema -p archipel-core` on a branch.
- **What an incoming `mcp/message` with `sessionId` does end to end.** I traced the code, not a run. I also did not check how the kits draw a pending request whose method is `mcp/message`. The React kit's request rendering (`items.tsx` around 255–290) handles only the approval and question shapes. Settle it with the test in step 1, plus a look at the kit's fallback for unknown methods.
- **Whether Claude's adapter sends `sessionId` on `mcp/message` in any version.** The watch outputs for this date contain no such change in the adapter's diff, and I did not read the adapter source. Settle it with the next adapter release or the live probe in step 5.
- **Whether the new `if`/`then` is enforced anywhere on our side.** It is not enforced by the Rust deserializer. Whether we would want it enforced is a design choice, not something the diff settles.
