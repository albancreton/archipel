# watch: what changed upstream

The library stands on things that move: the ACP v2 schema, the Claude adapter, the Claude Agent SDK, Claude Code, Pi and its adapter. This script looks for new versions of each, puts what changed next to what we use, and can hand that to an agent that writes the report an update starts from.

It never changes the repository's own files. An update stays a decision.

```
cd tools/watch
node watch.ts                       # free: versions, differences, the adapter asked what it can do
node watch.ts --live                # also the conformance scenarios that spend no model turn, on a new adapter
node watch.ts --live=all            # every scenario on a new adapter (a few cents for Claude)
node watch.ts --report              # also an agent's report per changed source, in docs/updates/
node watch.ts claude-adapter pi     # only some sources
```

Or `just watch …` from the root. It needs Node 24, `git`, `tar`, and for the adapter checks `npm` and `conformance/config.json`.

## What one run does

For each source in `sources.json`:

1. Asks the registry (npm or crates.io) for the newest released version. Not newer than `current`: done.
2. Downloads both versions into `target/watch/cache` and unpacks them. Nothing from a package is run in this step.
3. Keeps the files named in `files` and writes their difference to `target/watch/out/<date>/<source>.diff`.
4. Cuts the changelog to the versions in between (`<source>.changelog.md`).
5. For the ACP SDK: compares the two schema files by what the library uses: methods, types, and the state rules every client applies (`<source>.schema-changes.json`). It also says so if our pinned schema is not the one of `current`.
6. For an adapter (`probe`): installs the new version without running its install scripts, starts the old and the new one, and compares what each says about itself at `initialize`. No model is called.

`target/watch/out/<date>/summary.md` has everything found. A line is printed per source: `same`, `NEW` or `FAILED`.

`--live` then runs `conformance/run.ts` against the new adapter. Its verdicts (`same`, `CHANGED`, `FAILED`) are the most direct answer to "does it still behave as we recorded".

`--report` starts an agent per changed source with `brief.md` and the list of files written. The agent may only read. What it prints becomes `docs/updates/<date>-<source>.md`: what changed, what it touches here, proposed steps, and what it could not judge. The agent is the `agent` entry of `sources.json` (`claude -p` with read-only tools); `--agent="…"` uses another for one run. The report is as good as the model: read it as a first pass by someone who read the diff, not as a verdict.

## After an update

Set `current` in `sources.json` to the version the library now uses. That is the only state the script has.

## Adding a source

An entry in `sources.json`: `id`, `registry` (`npm` or `crates`), `package`, `current`, `why` (given to the agent), `files` (patterns: `*` within a folder, `**` across folders), and where they apply `changelog` (a URL, when the package does not carry its changelog), `schema` (an ACP schema inside the package) and `probe` (the agent in `conformance/config.json` that runs this adapter).

## Running it on a schedule

The script is the component; what runs it is yours: cron, a launchd timer, a CI schedule, or a scheduled agent. Daily is cheap: without `--report` nothing is paid, and with it an agent runs only for a source that has a new version. The script does not remember that it already reported a version, so a daily run with `--report` writes a report every day until `current` is updated; run it without `--report` on the schedule and with it by hand, or accept the repeats.

Nothing personal leaves the machine: the requests name the packages and carry a generic client name.
