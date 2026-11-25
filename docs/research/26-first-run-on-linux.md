# First run on Linux, and the systemd unit

2026-11-24. First step of the second plan, in the order agreed with the user that day: systemd, pass-through, SDK gaps with fault tests, components, UI tests and more live scenarios, watch script.

Until now nothing had been built or run anywhere but macOS, and `examples/service/archipeld.service` had been written from systemd's documentation without being loaded. The user offered a Linux machine (Pop!_OS 24.04, x86-64, Rust 1.95, systemd 255, Node 24 through nvm, Claude Code logged in).

## What was done

A copy of the repository at `73c72b9` (tracked files only) went into a throwaway folder there. Everything below used that folder, a daemon home inside it, and an adapter installed with `npm install -g --prefix` inside it. All of it was removed afterwards, as was the unit file.

| Step | Result |
|---|---|
| `cargo build --workspace` | Failed: `openssl-sys` found no OpenSSL headers. See below. Built after `apt install libssl-dev` |
| `cargo test --workspace` | All 97 pass |
| `systemd-analyze --user verify` on the filled-in unit | Accepted |
| `systemctl --user enable --now archipeld` | Running; socket and log made in the home folder |
| `archipel agents` | `claude  Claude Code  ready  0.112.0` |
| New session, switch to Haiku, one prompt | The turn ran and answered |
| `kill -9` of the daemon | systemd restarted it within two seconds; the session was still listed |
| `systemctl --user restart archipeld` | New process; `archipel show` printed the session's history |
| `kill -TERM` | Clean exit, result `success`, left stopped: `Restart=on-failure` does not restart a clean exit |
| `session/delete` | Session gone |
| `PATH=/usr/bin:/bin` in the unit | `archipel agents` fails with "could not start the agent: claude-agent-acp: No such file or directory" |
| `disable --now`, remove the file, `daemon-reload` | Nothing left |

The unit file needed no change.

## Found on the way

**Building on Linux needs the OpenSSL headers.** `archipel-transport` uses `tokio-tungstenite` with its `native-tls` feature, so that the Rust client can open `wss://` addresses. On macOS that is the system's own TLS and needs nothing. On Linux it is OpenSSL, and the build needs `libssl-dev` and `pkg-config`. The daemon itself never terminates TLS (`docs/design/daemon.md`: that is left to Tailscale or a reverse proxy), so only the client side needs it.

The other way would be rustls, which builds with no system package. It was looked at and not taken: in this version of the WebSocket library it comes without a crypto provider, so it means two more dependencies and code to choose one, and it moves away from "platform TLS" in the README's principles. One `apt install` is the smaller cost. `AGENTS.md` now names the packages. Worth looking at again if the library is ever published as a crate, since a consumer's build machine would then need the headers too.

**The adapter has moved again**: a fresh install gave 0.112.0 (baselines are on 0.109.1; 0.110.1 was checked in full the day before). Only the one turn above was run on 0.112.0, not the conformance suite.

**Claude Code leaves an empty project folder behind** after a session is deleted (`~/.claude/projects/<cwd>/memory`). Not ours to clean up in the daemon; removed by hand here.

## Not checked

- Linux on ARM, and any distribution other than this one.
- `loginctl enable-linger`: it was already on for this user, so the service running while logged out was not shown to depend on it.
- The TypeScript and Swift tests on Linux.

Cost: one Haiku turn.
