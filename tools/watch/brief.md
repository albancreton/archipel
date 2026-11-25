You are preparing an update of the archipel library to a new version of something it depends on. You are in the repository's root. Do not change any file: your answer is the report, in Markdown, and nothing else. Start with the heading, without a preamble.

Read first: `AGENTS.md`, `schema/README.md`, `docs/design/daemon.md` (the parts on ACP, addons and profiles), and the files named under "What was found" below. Then read the code of ours that the changes touch. Read the diff itself, not only its summary; it may be long, so search it for what matters (the names of our message kinds, methods and capabilities) instead of reading it from top to bottom.

The report has these sections, in this order:

# <source>: <old version> to <new version>

## In short
Three to six lines: what changed upstream that matters to this library, and how much work the update is. If nothing matters to us, say so plainly and stop after "What I could not judge".

## What changed upstream
Only what is in the diff or the changelog. Group by theme. Quote names exactly.

## What it touches here
For each change that matters: the file or behaviour of ours it touches, and why. Look in particular at:
- `schema/acp-v2.schema.json` and the generated state rules (`schema/state-rules.json`): a changed rule changes the state every client holds.
- The addons in `crates/archipel-addons`: each fixes a fault of an adapter or a harness. Say if upstream has repaired a fault an addon works around (then the addon can go), or changed what an addon relies on.
- The profiles in `crates/archipel-agents` (Claude, and the Pi translation from ACP v1).
- The recordings in `testdata/transcripts` and the conformance baseline in `conformance/baseline`: which would have to be made again.
- The client SDKs (`clients/ts`, `clients/swift`) and the kits (`ui/react`, `ui/swift`): new kinds of item or capability they could show.

## Proposed steps
A numbered list, smallest safe step first, each with how to check it (which test or which conformance scenario).

## What I could not judge
Everything you are unsure of, and what would settle it. Do not guess: a short honest list here is worth more than a confident wrong claim above.

Write plainly, in full sentences, for a reader who knows the library but has not seen the diff.
