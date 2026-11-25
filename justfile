# Run `just` to list the commands. Install: `brew install just` or `cargo install just`.

default:
    @just --list

# Regenerate everything derived from the schema: the rules, the extension schema, client types.
gen:
    cargo run -q -p archipel-schema --bin gen
    cd clients/ts && node generate-types.mjs

# Rewrite the expected renderings in testdata/vectors after a deliberate change. Review the diff.
vectors:
    UPDATE_VECTORS=1 cargo test -q -p archipel-agents --test pi
    UPDATE_VECTORS=1 cargo test -q -p archipel-core --test transcripts
    UPDATE_VECTORS=1 cargo test -q -p archipel-server --test daemon every_recorded

# Start a daemon with a fake agent that replays a recorded session. Then use `target/debug/archipel --home target/demo-home …`.
demo scenario="approval_command":
    cargo build -q
    mkdir -p target/demo-home
    target/debug/archipeld --home target/demo-home --fake testdata/transcripts/claude-v2/{{scenario}}.ndjson

# Format, lint and type-check everything.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    pnpm -r check

# Run all tests: Rust, TypeScript, Swift. The browser tests need `npx playwright install chromium-headless-shell` once.
test:
    cargo test --workspace
    pnpm -r test
    cd ui/react && pnpm test:browser
    swift test

# Format the code.
fmt:
    cargo fmt --all

# What CI runs: generated code must be up to date, then check and test.
ci: gen
    git diff --exit-code
    just check
    just test

# Live checks against a real harness, compared with conformance/baseline. Costs a few cents for Claude.
conformance agent *args:
    cd conformance && node run.ts {{agent}} {{args}}

# Which lines of the addon system do the tests run? Fails if one is neither run nor accepted with a reason.
addon-coverage:
    scripts/addon-coverage.sh

# Break the addon system's rules one at a time and check that the tests notice. Takes a minute.
addon-mutations:
    scripts/addon-mutations.py

# Look for new versions of what the library depends on and write down what changed. Free; see tools/watch/README.md.
watch *args:
    cd tools/watch && node watch.ts {{args}}

# Take what is personal out of recordings and logs before committing them. `just scrub --check testdata` only looks.
scrub *args:
    scripts/scrub-recordings.py {{args}}
