#!/usr/bin/env python3
"""Upstream watch for archipel, as a Hermes cron monitor script.

Install: copy to ~/.hermes/scripts/ and name it with `--monitor-script` (see README.md here).

Runs archipel's watch script (free: no model is called) and prints one line per source that
has a newer upstream version than the repository uses:

    <source>\t<version in use>\t<newest version>

Hermes hashes these exact bytes each tick. The same lines as last time: nothing happens and no
agent runs. A new or changed line: the agent is woken with the difference and follows the
skill `archipel-upstream-watch`. So the output must not vary by itself: no dates, sorted.

A failure (no network, a registry down) exits with an error, which Hermes reports as a failed
run and never counts as a change.

The repository is at ~/Dev/archipel unless ARCHIPEL_REPO says otherwise. It is updated first
(`git pull --ff-only`), so the versions "in use" follow what the repository says.
"""

import glob
import json
import os
import subprocess
import sys

repo = os.environ.get("ARCHIPEL_REPO", os.path.expanduser("~/Dev/archipel"))

# Node installed with nvm is not on the PATH of a service.
env = dict(os.environ)
nodes = sorted(glob.glob(os.path.expanduser("~/.nvm/versions/node/*/bin")))
if nodes:
    env["PATH"] = nodes[-1] + os.pathsep + env.get("PATH", "")

# Not being able to update is not a reason to stop: the watch still says what is new.
subprocess.run(["git", "-C", repo, "pull", "--ff-only", "--quiet"], env=env, capture_output=True, timeout=120)

run = subprocess.run(["node", "watch.ts", "--json"], cwd=os.path.join(repo, "tools/watch"), env=env, capture_output=True, text=True, timeout=1800)
try:
    result = json.loads(run.stdout)
except ValueError:
    sys.stderr.write("the watch script did not answer:\n" + (run.stderr or run.stdout)[-2000:])
    sys.exit(1)
if result.get("failed"):
    sys.stderr.write("the watch script could not look at: " + json.dumps(result["failed"]))
    sys.exit(1)

lines = sorted("%s\t%s\t%s" % (source["id"], source["from"], source["to"]) for source in result["sources"])
print("\n".join(lines) if lines else "everything is at the newest version")
