# Running the daemon as a service

Use this when sessions should go on while apps come and go, or when the machine is reached remotely: the operating system starts `bridged` at login and restarts it if it stops.

`bridged` already runs the way a service manager wants: in the foreground, logging to stderr, stopping cleanly on SIGTERM. So a service is one small file. Two are here, with three placeholders to fill in:

| Placeholder | What goes there |
|---|---|
| `__BRIDGED__` | The full path of the daemon program, for example the output of `command -v bridged`. Your own build if you have addons of your own (`examples/own-daemon`) |
| `__PATH__` | The `PATH` the daemon runs with. It must contain the adapters (`claude-agent-acp`, `pi-acp`), `node`, and the harnesses (`claude`, `pi`). Your shell's `$PATH` is the simple choice |
| `__HOME__` | Your home folder (macOS file only, for the log) |

The `PATH` line is the one that goes wrong. A service does not get your shell's `PATH`, so without it the daemon starts and every agent fails with "could not start the agent".

## macOS

```sh
sed -e "s|__BRIDGED__|$(command -v bridged)|" -e "s|__PATH__|$PATH|" -e "s|__HOME__|$HOME|" \
  dev.harness-bridge.bridged.plist > ~/Library/LaunchAgents/dev.harness-bridge.bridged.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.harness-bridge.bridged.plist

launchctl print gui/$(id -u)/dev.harness-bridge.bridged | head     # is it running?
tail -f ~/Library/Logs/harness-bridge.log                           # what it says
launchctl bootout gui/$(id -u)/dev.harness-bridge.bridged           # stop and unload
```

## Linux

```sh
mkdir -p ~/.config/systemd/user
sed -e "s|__BRIDGED__|$(command -v bridged)|" -e "s|__PATH__|$PATH|" \
  bridged.service > ~/.config/systemd/user/bridged.service
systemctl --user daemon-reload
systemctl --user enable --now bridged

systemctl --user status bridged          # is it running?
journalctl --user -u bridged -f          # what it says
systemctl --user disable --now bridged   # stop
loginctl enable-linger "$USER"           # optional: keep it running when you are logged out
```

## Things to know

- **After replacing the program, restart the service.** It keeps running the old one until then: `launchctl kickstart -k gui/$(id -u)/dev.harness-bridge.bridged`, or `systemctl --user restart bridged`. A restart interrupts running turns; the sessions and their history stay.
- **The daemon's files** are in `~/.harness-bridge` (`bridged --help` lists them). Add `--home DIR` to the program arguments for another place.
- **One daemon per home folder.** An app that starts the daemon itself (`examples/node/daemon.ts`) first checks whether one answers, and then uses the service.
- **Remote access** (`--ws ADDR`) is one more argument: see `docs/remote.md`.
- **Windows** is not covered: the daemon does not run there yet.

How far this was checked: the macOS file passes `plutil -lint` in the tests. Neither file was loaded into a service manager while writing this.
