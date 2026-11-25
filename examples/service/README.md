# Running the daemon as a service

Use this when sessions should go on while apps come and go, or when the machine is reached remotely: the operating system starts `archipeld` at login and restarts it if it stops.

`archipeld` already runs the way a service manager wants: in the foreground, logging to stderr, stopping cleanly on SIGTERM. So a service is one small file. Two are here, with three placeholders to fill in:

| Placeholder | What goes there |
|---|---|
| `__ARCHIPELD__` | The full path of the daemon program, for example the output of `command -v archipeld`. Your own build if you have addons of your own (`examples/own-daemon`) |
| `__PATH__` | The `PATH` the daemon runs with. It must contain the adapters (`claude-agent-acp`, `pi-acp`), `node`, and the harnesses (`claude`, `pi`). Your shell's `$PATH` is the simple choice |
| `__HOME__` | Your home folder (macOS file only, for the log) |

The `PATH` line is the one that goes wrong. A service does not get your shell's `PATH`, so without it the daemon starts and every agent fails with "could not start the agent".

## macOS

```sh
sed -e "s|__ARCHIPELD__|$(command -v archipeld)|" -e "s|__PATH__|$PATH|" -e "s|__HOME__|$HOME|" \
  dev.archipel.archipeld.plist > ~/Library/LaunchAgents/dev.archipel.archipeld.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.archipel.archipeld.plist

launchctl print gui/$(id -u)/dev.archipel.archipeld | head     # is it running?
tail -f ~/Library/Logs/archipel.log                           # what it says
launchctl bootout gui/$(id -u)/dev.archipel.archipeld           # stop and unload
```

## Linux

```sh
mkdir -p ~/.config/systemd/user
sed -e "s|__ARCHIPELD__|$(command -v archipeld)|" -e "s|__PATH__|$PATH|" \
  archipeld.service > ~/.config/systemd/user/archipeld.service
systemctl --user daemon-reload
systemctl --user enable --now archipeld

systemctl --user status archipeld          # is it running?
journalctl --user -u archipeld -f          # what it says
systemctl --user disable --now archipeld   # stop
loginctl enable-linger "$USER"           # optional: keep it running when you are logged out
```

## Things to know

- **After replacing the program, restart the service.** It keeps running the old one until then: `launchctl kickstart -k gui/$(id -u)/dev.archipel.archipeld`, or `systemctl --user restart archipeld`. A restart interrupts running turns; the sessions and their history stay.
- **The daemon's files** are in `~/.archipel` (`archipeld --help` lists them). Add `--home DIR` to the program arguments for another place.
- **One daemon per home folder.** An app that starts the daemon itself (`examples/node/daemon.ts`, `examples/swift/Daemon.swift`) first checks whether one answers, and then uses the service.
- **Remote access** (`--ws ADDR`) is one more argument: see `docs/remote.md`.
- **systemd stops trying** after five failed starts within ten seconds (its default, not something this unit sets). `systemctl --user reset-failed archipeld` clears that.
- **The adapters and harnesses stop with the service** on Linux: systemd ends every process the daemon started.
- **Windows** is not covered: the daemon does not run there yet.

How far this was checked:

- **macOS: run by hand on 2026-11-23** (macOS 27, on a test home folder). Loaded with the commands above, the daemon started and a Claude session was made and deleted through it. Killed with `kill -9`, launchd started it again. `launchctl kickstart -k` restarted it. `bootout` stopped and removed it. With `PATH` set to `/usr/bin:/bin`, the agent failed with "could not start the agent: node: No such file or directory", as described above.
- **Linux: run by hand on 2026-11-24** (Pop!_OS 24.04, systemd 255, on a test home folder). `systemd-analyze --user verify` accepted the unit. Enabled with the commands above, the daemon started, `archipel agents` reported Claude ready, and a Claude session ran a turn and was deleted through it. Killed with `kill -9`, systemd started it again and the session was still listed. `systemctl --user restart` restarted it. A `SIGTERM` ended it cleanly and systemd left it stopped, as `Restart=on-failure` says. With `PATH` set to `/usr/bin:/bin`, the agent failed with "could not start the agent: claude-agent-acp: No such file or directory". `disable --now` and removing the file left nothing behind.
- The tests only lint the macOS file (`plutil -lint`).
