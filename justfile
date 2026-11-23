# Run `just` to list the commands. Install: `brew install just` or `cargo install just`.

default:
    @just --list

# Regenerate schema/state-rules.json and schema/bridge.schema.json.
gen:
    cargo run -q -p bridge-schema --bin gen

# Rewrite the expected renderings in testdata/vectors after a deliberate change. Review the diff.
vectors:
    UPDATE_VECTORS=1 cargo test -q -p bridge-core --test transcripts

# Format, lint and type-check everything.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests.
test:
    cargo test --workspace

# Format the code.
fmt:
    cargo fmt --all

# What CI runs: generated code must be up to date, then check and test.
ci: gen
    git diff --exit-code
    just check
    just test
