# Running the watch from Hermes

[Hermes Agent](https://hermes-agent.nousresearch.com/docs) can run recurring jobs and tell you on a messaging platform when one finds something. This folder holds what such a job needs. Any other scheduler works too; see `../README.md`.

How it fits together:

- `archipel_watch.py` is a Hermes **monitor script**. It runs the watch script (free) and prints one line per source that has a newer version. Hermes compares that output with last time's; while it is the same, no agent runs and nothing is sent.
- When a line appears or changes, Hermes wakes its agent with the difference. The skill in `SKILL.md` tells it to write the report that `../brief.md` describes and to answer with a short message, which Hermes delivers.

Setup, on the machine where Hermes runs:

```
git clone https://github.com/albancreton/archipel ~/Dev/archipel
cd ~/Dev/archipel && pnpm install                      # Node 24
cp tools/watch/hermes/archipel_watch.py ~/.hermes/scripts/
mkdir -p ~/.hermes/skills/software-development/archipel-upstream-watch
cp tools/watch/hermes/SKILL.md ~/.hermes/skills/software-development/archipel-upstream-watch/
python3 ~/.hermes/scripts/archipel_watch.py            # once by hand: downloads what it compares; prints the lines

hermes cron create "0 8 * * *" \
  "Follow the skill archipel-upstream-watch. The monitor output lists the upstream sources of the archipel library that have a newer version than the repository uses; the difference above says which lines are new." \
  --name "archipel upstream watch" --skill archipel-upstream-watch \
  --monitor-script archipel_watch.py --workdir "$HOME/Dev/archipel" --deliver discord
```

`--deliver discord` sends to the home channel; `discord:<channel id>` to another.

After an update of the library, `current` in `tools/watch/sources.json` changes, the monitor's lines change with it, and the agent answers `[SILENT]` when lines only disappeared.

The repository is elsewhere: set `ARCHIPEL_REPO` for the script and change `--workdir`.
