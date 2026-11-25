# Conformance: what the real harnesses do

Live checks. Each scenario does something with a real harness through the daemon, on the cheapest model, and notes what it saw. What it saw is compared with a baseline, so a new version of Claude Code, of an adapter or of Pi that behaves differently shows up as `CHANGED`.

This is the third test tier of `docs/research/21-second-plan.md`. The recorded sessions in `testdata/` say what the library does with known input; this says whether the input is still what we think it is.

```
cargo build
cp conformance/config.example.json conformance/config.json     # then fill in the paths
cd conformance
node run.ts claude                 # every scenario: about forty short Haiku turns, a few cents
node run.ts claude --free          # only the scenarios that spend no model turn
node run.ts pi                     # free on a local model
node run.ts claude reopen.keeps_settings turn.interrupt       # some scenarios
node run.ts claude --update        # write what was seen as the new baseline
```

It prints one line per scenario and writes `reports/<agent>.md` with everything seen. The exit code is 1 if a scenario failed or changed.

| Result | Meaning |
|---|---|
| `same` | What was seen matches the baseline |
| `CHANGED` | It differs. Either the harness changed or the scenario did: read the report, then `--update` if the new behaviour is the one to expect |
| `FAILED` | Something that must hold whatever the version did not, or the scenario could not finish |
| `new` | No baseline yet for this scenario |

How it is built:

- `scenarios.ts` holds the scenarios. A scenario notes **facts** (compared with the baseline), **info** (durations, lists that change with every release; reported, not compared) and what **must** hold.
- Nothing depends on the wording of an answer, beyond a marker word the prompt asks for.
- `baseline/<agent>.json` is the record of how the harness behaved when last looked at. It is committed; a change to it is a change in what we rely on.
- The run starts a daemon of its own in `target/conformance/<agent>` and works in `conformance/work/<agent>`. It does not touch any other daemon.
- For Claude it deletes every session it made from Claude's own history at the end. Pi's adapter cannot delete: the sessions stay in Pi's folder for `conformance/work/pi`.

Addons (`docs/design/addons.md`) run as shipped. A scenario that checks a fix also notes what the harness does with that addon switched off (`t.restartDaemon({ "<id>": false })`): `reopen.keeps_settings.unaided`, and the fact `unknownEffortUnaided`. When one of those shows `CHANGED` towards the right behaviour, upstream has repaired the fault and the addon can be deleted.

`session.mcp_server` gives the session `mcp-marker.mjs`, a small MCP server with one tool, written without a dependency.

Some scenarios also talk to the adapter directly (`session.empty`, `session.options`, `session.rename_delete`, `session.rename_used`), to compare what the harness does with what arrives through the daemon.
