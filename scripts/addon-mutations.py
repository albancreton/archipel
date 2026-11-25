#!/usr/bin/env python3
"""Do the addon tests notice when a rule of the addon system is broken?

Line coverage (scripts/addon-coverage.sh) says a line ran, not that a test would fail if
it were wrong. This breaks one rule at a time in the daemon, runs the addon tests, and
expects them to fail. It restores the file afterwards.

A mutation whose text is no longer in the daemon is reported as STALE: rewrite it here.
Exits 1 if a broken rule goes unnoticed or a mutation is stale.
"""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DAEMON = ROOT / "crates/archipel-server/src/daemon.rs"
TESTS = "cargo test -q -p archipel-server --test addons --test app_side 2>&1 | tail -40"

# (the rule that is broken, the text in the daemon, what it becomes)
MUTATIONS = [
 ("layer order towards the adapter reversed", "            What::Notification(_) => (next..layers).collect(),", "            What::Notification(_) => (next..layers).rev().collect(),"),
 ("an addon is shown its own request", "                self.dispatch(agent_id, index + 1, sent);", "                self.dispatch(agent_id, index, sent);"),
 ("release by another addon allowed", "        if held.addon != index {\n            self.held.insert(id, held);\n            return;\n        }", ""),
 ("held response released skips nothing but loses new outcome", "            Self::Pass(Some(result)) => addon::Outcome::Result(result),", "            Self::Pass(Some(_)) => was,"),
 ("answer to an addon's question passes the asker's own layer", "            Asker::Addon { addon, .. } => addon,\n        };\n        for index in from..end", "            Asker::Addon { .. } => self.addons.len(),\n        };\n        for index in from..end"),
 ("app-side response hooks run in request order", "            (0..next.min(layers)).rev().collect()\n        };\n        for index in order {\n            let about", "            (0..next.min(layers)).collect()\n        };\n        for index in order {\n            let about"),
 ("app request answered by addon also reaches the daemon", "            if matches!(edge.kind, EdgeKind::Request) {\n                self.edge_answer(index, edge, &outcome);\n            }\n            return;", "            if matches!(edge.kind, EdgeKind::Request) {\n                self.edge_answer(index, edge, &outcome);\n                return;\n            }"),
 ("addon mark missing on its updates", "                self.mark(index, &mut update);", ""),
 ("held approval logged while held", "                Some(addon::Verdict::Hold) => {\n                    return self.hold(index, agent_id, ask.request, HeldWhat::Ask(ask.into()));", "                Some(addon::Verdict::Hold) => {\n                    self.log_ask(&ask);\n                    return self.hold(index, agent_id, ask.request, HeldWhat::Ask(ask.into()));"),
 ("stopped hook not called", "                addon.stopped(cx, &agent, why);", ""),
 ("announce with a session goes to every app", "                    .map(|session| session.subscribers.iter().copied().collect());", "                    .map(|_| self.clients.keys().copied().collect());"),
 ("app_call passes the app-side hooks", "                self.handle(client, wire::request(id, &method, params));", "                self.client_message(client, wire::request(id, &method, params));"),
]


def main() -> int:
    original = DAEMON.read_text()
    unnoticed = 0
    try:
        for rule, old, new in MUTATIONS:
            if original.count(old) != 1:
                print(f"STALE      {rule}")
                unnoticed += 1
                continue
            DAEMON.write_text(original.replace(old, new))
            out = subprocess.run(TESTS, shell=True, capture_output=True, text=True, cwd=ROOT).stdout
            if "could not compile" in out:
                print(f"NO BUILD   {rule}")
                unnoticed += 1
            elif "FAILED" in out:
                print(f"noticed    {rule}")
            else:
                print(f"UNNOTICED  {rule}")
                unnoticed += 1
    finally:
        DAEMON.write_text(original)
    print(f"\n{len(MUTATIONS) - unnoticed} of {len(MUTATIONS)} broken rules noticed by the tests")
    return 1 if unnoticed else 0


if __name__ == "__main__":
    sys.exit(main())
