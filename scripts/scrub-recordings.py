#!/usr/bin/env python3
"""Takes what is personal out of recorded sessions and logs before they are committed.

    scripts/scrub-recordings.py [--check] [--word NAME=REPLACEMENT ...] PATH ...

A recording holds whatever the harness said on the machine it was made on: the home folder
and the project's path, the account's plan, the names of local model servers, and the user's
own commands and plugins. This replaces them with neutral values, in plain text and inside
base64 (a command's output is recorded that way).

- The home folder and the user's name come from the environment, so no name is written here.
- `--word lan-box=local` replaces a word of your own, such as a machine's name.
- `--check` changes nothing and exits with 1 if something would change.

`.ndjson` and `.json` files are read as JSON, so the account and the command list are found by
their place. Other files get the text replacements only. After scrubbing `testdata/`, run
`just vectors`: the expected states and renderings are made from the recordings.
"""
import base64, binascii, json, os, re, sys

args = sys.argv[1:]
check = "--check" in args
words = []
paths = []
it = iter(a for a in args if a != "--check")
for a in it:
    if a == "--word":
        old, new = next(it).split("=", 1)
        # Not inside a longer word; an underscore before it is fine (`mcp__name`).
        words.append((re.compile(r"(?<![A-Za-z0-9])%s(?![A-Za-z0-9])" % re.escape(old)), new))
    else:
        paths.append(a)

user = os.environ.get("USER", "")
home = os.path.expanduser("~")
dashed = home.replace("/", "-")  # how Claude Code names a project's folder
rules = [
    # The scratch project the recordings were made in, then any other path under the home folder.
    (re.compile(re.escape(dashed) + r"-Dev-[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*?-(?:spikes-acp-probe|tools-record)-work"), "-work-probe"),
    (re.compile(re.escape(home) + r"/Dev/[A-Za-z0-9-]+/(?:spikes/acp-probe|tools/record)/work"), "/work/probe"),
    (re.compile(re.escape(home) + r"/Dev/([A-Za-z0-9-]+)"), r"/work/\1"),
    (re.compile(re.escape(dashed) + r"-Dev-"), "-work-"),
    (re.compile(re.escape(home)), "/Users/dev"),
    (re.compile(re.escape(dashed) + r"(?=-)"), "-Users-dev"),
    (re.compile(r"\bClaude (Ultra|Max|Pro|Team)\b"), "Claude Plan"),
] + words
if user:
    rules.append((re.compile(r"(?<![A-Za-z0-9])%s(?![A-Za-z0-9])" % re.escape(user)), "dev"))

def text(value):
    for pattern, new in rules:
        value = pattern.sub(new, value)
    return value

B64 = re.compile(r"^[A-Za-z0-9+/]{16,}={0,2}$")

def string(value):
    """A string, scrubbed; base64 that holds text is opened, scrubbed and closed again."""
    if B64.match(value) and len(value) % 4 == 0:
        try:
            inner = base64.b64decode(value, validate=True).decode("utf8")
        except (binascii.Error, UnicodeDecodeError):
            return text(value)
        cleaned = text(inner)
        return value if cleaned == inner else base64.b64encode(cleaned.encode("utf8")).decode("ascii")
    return text(value)

def personal_command(entry):
    """A command that comes from a plugin or is the user's own, not the harness's."""
    name = entry.get("name") if isinstance(entry, dict) else None
    return isinstance(name, str) and (":" in name or name in OWN_COMMANDS)

OWN_COMMANDS = set(os.environ.get("SCRUB_COMMANDS", "").split(",")) - {""}

def walk(value, key=None):
    if isinstance(value, str):
        # A string that is itself JSON (a harness passes some results that way).
        if value[:1] in "{[" and value[-1:] in "}]":
            try:
                inner = json.loads(value)
            except ValueError:
                return string(value)
            cleaned = walk(inner)
            return value if cleaned == inner else json.dumps(cleaned, ensure_ascii=False, separators=(",", ":"))
        return string(value)
    if isinstance(value, list):
        if key in ("availableCommands", "commands"):
            value = [entry for entry in value if not personal_command(entry)]
        return [walk(entry) for entry in value]
    if isinstance(value, dict):
        out = {}
        for name, inner in value.items():
            # The account's plan, wherever an account is described.
            if name == "plan" and isinstance(inner, str) and ("email" in value or "organization" in value or value.get("kind") == "account"):
                out[name] = "subscription"
            else:
                out[text(name)] = walk(inner, name)
        return out
    return value

def scrub(path):
    with open(path, encoding="utf8") as file:
        before = file.read()
    if path.endswith(".ndjson"):
        lines = []
        for line in before.split("\n"):
            try:
                lines.append(json.dumps(walk(json.loads(line)), ensure_ascii=False, separators=(",", ":")) if line.strip() else line)
            except ValueError:
                lines.append(text(line))
        after = "\n".join(lines)
    elif path.endswith(".json"):
        try:
            data = json.loads(before)
            cleaned = walk(data)
            # A file with nothing to scrub keeps its own formatting.
            after = before if cleaned == data else json.dumps(cleaned, ensure_ascii=False, indent=2) + "\n"
        except ValueError:
            after = text(before)
    else:
        after = text(before)
    if after != before and not check:
        with open(path, "w", encoding="utf8") as file:
            file.write(after)
    return after != before

changed = []
for root in paths:
    found = [root] if os.path.isfile(root) else [os.path.join(folder, name) for folder, _, names in os.walk(root) for name in names]
    for path in sorted(found):
        if "/node_modules/" in path or "/.git/" in path:
            continue
        try:
            if scrub(path):
                changed.append(path)
        except UnicodeDecodeError:
            pass
for path in changed:
    print(("would change " if check else "scrubbed ") + path)
sys.exit(1 if check and changed else 0)
