# crates

The Rust workspace. See `docs/design/daemon.md`, section 12, for what each crate is for. `archipel-ffi` (C interface for embedding) is planned and not scaffolded.

Where to start reading:

- `archipel-server/src/daemon.rs`: the daemon. It opens with a table of what each message causes.
- `archipel-server/tests/daemon.rs`: the daemon run against recorded sessions. The test names say what is guaranteed.
- `archipel-core/src/mirror.rs`: what a client holds and how the daemon's messages change it.

Programs: `archipeld` (the daemon) and `archipel` (a command-line client, in `archipel-client`). Both have `--help`.
