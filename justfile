# Run `just` to list the commands. Install: `brew install just` or `cargo install just`.

default:
    @just --list

# Regenerate types from schema/ (nothing to generate yet).
gen:
    @echo "gen: no generators wired yet"

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
