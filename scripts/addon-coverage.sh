#!/bin/bash
# Which lines of the addon system do the tests run?
#
# Runs the Rust tests with the compiler's coverage instrumentation and lists every line of
# the addon code that no test ran. Exits 1 if there is one that is not marked as accepted.
#
# The addon code is: the interface crate, the shipped addons, and the functions of the
# daemon named in ADDON_FUNCTIONS below. A line is accepted with a comment on it or on the
# line above that starts with `cov:` and says why no test reaches it.
#
# Needs the toolchain's own LLVM tools, once: rustup component add llvm-tools
set -euo pipefail
cd "$(dirname "$0")/.."

TOOLS="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"
if [ ! -x "$TOOLS/llvm-cov" ]; then
  echo "llvm-cov is missing: run 'rustup component add llvm-tools'" >&2
  exit 2
fi

OUT=target/addon-coverage
rm -rf "$OUT" && mkdir -p "$OUT"
RUSTFLAGS="-C instrument-coverage" LLVM_PROFILE_FILE="$PWD/$OUT/%p-%m.profraw" \
  CARGO_TARGET_DIR=target/coverage-build \
  cargo test -q -p archipel-addon-api -p archipel-addons -p archipel-server >"$OUT/tests.log" 2>&1 \
  || { cat "$OUT/tests.log"; exit 1; }
"$TOOLS/llvm-profdata" merge -sparse "$OUT"/*.profraw -o "$OUT/all.profdata"

OBJECTS=()
for file in target/coverage-build/debug/deps/{addons,app_side,daemon,delete,archipel_addons,archipel_addon_api}-*; do
  case "$file" in *.d | *.o | *.rmeta | *.rlib | *.dSYM) ;; *) OBJECTS+=(-object "$file") ;; esac
done
SOURCES=(
  "$PWD/crates/archipel-addon-api/src"
  "$PWD/crates/archipel-addons/src"
  "$PWD/crates/archipel-server/src/daemon.rs"
)
"$TOOLS/llvm-cov" report "${OBJECTS[@]}" -instr-profile="$OUT/all.profdata" -sources "${SOURCES[@]}" 2>/dev/null |
  awk 'NR > 2 && /\.rs|TOTAL/ {printf "%-50s lines %7s   regions %7s\n", $1, $10, $4}'
"$TOOLS/llvm-cov" show "${OBJECTS[@]}" -instr-profile="$OUT/all.profdata" -sources "${SOURCES[@]}" \
  >"$OUT/lines.txt" 2>/dev/null

python3 - "$OUT/lines.txt" <<'PY'
import re, sys

# The daemon's functions that are the addon system. The rest of daemon.rs is the core.
ADDON_FUNCTIONS = """
own_updates last_update updates session inward scope_name step_of_flow step_of_keep
send answer_addon client_message edge edge_answer agent_for app serve
agent_view addon_id hook origin source mark effect hold release dispatch settle pass
ask log_ask answered_by answered deliver over cancelled
""".split()

file = function = previous = None
missed = accepted = 0
for line in open(sys.argv[1]):
    if line.startswith("/") and line.rstrip().endswith(":"):
        file, function, previous = line.strip().rstrip(":").split("crates/")[-1], None, ""
        continue
    match = re.match(r"\s*(\d+)\|\s*([0-9.kM]*)\|(.*)", line)
    if not match:
        continue
    number, count, source = match.groups()
    if source.startswith("#[cfg(test)]"):
        file = None  # the rest of the file is its own tests
    if file is None:
        continue
    named = re.search(r"\bfn (\w+)", source)
    if named:
        function = named.group(1)
    whole_file = not file.endswith("daemon.rs")
    if count == "0" and (whole_file or function in ADDON_FUNCTIONS):
        if "cov:" in source or "cov:" in previous:
            accepted += 1
        else:
            missed += 1
            print(f"not run: {file}:{number} [{function}] {source.strip()[:90]}")
    previous = source
print(f"\nlines of the addon system no test runs: {missed} (accepted with a reason: {accepted})")
sys.exit(1 if missed else 0)
PY
