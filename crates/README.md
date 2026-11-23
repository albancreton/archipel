# crates

The Rust workspace. See `docs/design/daemon.md`, section 12, for what each crate is for. `bridge-ffi` (C interface for embedding) is planned and not scaffolded.

Where to start reading:

- `bridge-server/src/daemon.rs`: the daemon. It opens with a table of what each message causes.
- `bridge-server/tests/daemon.rs`: the daemon run against recorded sessions. The test names say what is guaranteed.
- `bridge-core/src/mirror.rs`: what a client holds and how the daemon's messages change it.

Programs: `bridged` (the daemon) and `bridge` (a command-line client, in `bridge-client`). Both have `--help`.
