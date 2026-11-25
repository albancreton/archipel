# The name, and getting ready to be public

2026-11-24. The user chose the name **archipel** and a repository for it. Until now the project was called harness-bridge, a placeholder, and had no remote.

## The rename

Agreed scope: everything, since nothing has shipped and nothing breaks.

| Was | Is |
|---|---|
| `harness-bridge` (project, folder, home `~/.harness-bridge`) | `archipel`, `~/.archipel` |
| Crates `bridge-core`, `bridge-server`, … | `archipel-core`, `archipel-server`, … |
| Programs `bridged`, `bridge` | `archipeld`, `archipel` |
| Packages `@harness-bridge/client`, `@harness-bridge/react` | `@archipel/client`, `@archipel/react` |
| Swift package `HarnessBridge`, modules `BridgeClient`, `BridgeUI` | `Archipel`, `ArchipelClient`, `ArchipelUI` |
| Types `BridgeClient`, `BridgeSession`, the `Bridge` namespace | `ArchipelClient`, `ArchipelSession`, `Archipel` |
| Wire: `_bridge/…` methods, `_meta.bridge` | `_archipel/…`, `_meta.archipel` |
| `BRIDGE_HOME` | `ARCHIPEL_HOME` |
| CSS classes `hb-…` | `arc-…` |
| Service files `dev.harness-bridge.bridged.plist`, `bridged.service` | `dev.archipel.archipeld.plist`, `archipeld.service` |

Done by a script over every tracked text file and path, then checked by building and running everything: 106 Rust tests, 113 TypeScript, 13 in the browser, and the Swift suites pass. The research notes were renamed too, so the commands in them still work; where they say "bridge" as a plain word (an "MCP bridge", "a Pi bridge") it was left.

Not renamed: `spikes/` (raw material of the first experiments) and the commits before this one.

The names `@archipel/…` on npm and `archipel-…` on crates.io were **not checked for availability**. That matters only at publishing.

## What was personal in the repository

The target repository is public, and the recordings held what the harnesses said on the machine they were made on:

- the home folder and the project's path, in plain text and inside base64 (a command's output is recorded that way);
- the account's plan;
- the name of a local model server;
- the user's own commands and the commands of installed plugins, with their descriptions;
- paths of a temporary folder in a few research notes.

`scripts/scrub-recordings.py` takes these out, and `just vectors` remade the states, renderings and daemon recordings from the scrubbed sources. The script reads the home folder and the user's name from the environment, so it holds no name itself; it is kept for the next recording.

What is still there and was judged harmless: the names of tools a harness listed (a browser tool, MCP servers of plugins in two spike logs), usage and rate-limit numbers, and the fixture prompts.

## Still open before anything is pushed

- **The history.** The scrub changes the files as they are now. The 60 or so earlier commits still hold the unscrubbed recordings, and every commit carries the author's machine address. Pushing the history publishes both. The choices are in the conversation of this date; nothing was pushed.
