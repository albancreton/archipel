# Reaching a daemon from another machine

The daemon can run commands as you. So it listens on one thing only: a local socket that only you can open. Nothing listens on the network, and reaching the daemon from elsewhere means getting onto its machine as you, which is what SSH is for.

**SSH is the way in from another machine.** It does the encryption, checks that the machine is the right one, and decides who gets in. The daemon adds no door of its own: it is exactly as reachable as your shell, and someone who cannot SSH to the machine cannot reach its daemon.

## 1. Through SSH

Nothing to configure on the daemon. If you can SSH to the machine, you can use its daemon:

```
archipel --via "ssh myhost archipel connect --stdio" sessions
```

`archipel connect --stdio` runs on the remote machine and carries one connection to the local socket on its input and output. The messages are the local socket's: one JSON message per line.

| From | Use |
|---|---|
| Rust | `Endpoint::Command("ssh myhost archipel connect --stdio")` |
| TypeScript (Node, Electron) | `commandTransport("ssh", ["myhost", "archipel", "connect", "--stdio"])` |
| Swift on macOS | `CommandTransport(command: "ssh", arguments: ["myhost", "archipel", "connect", "--stdio"])` |
| Swift on iOS, or any app with an SSH library of its own | `StreamTransport`: the app opens a channel that runs `archipel connect --stdio` and hands over its output and a way to write to it (`examples/swift-ssh` does it with Citadel) |

Things to know:

- **A command run through SSH does not have your terminal's `PATH`.** If `archipel` is not found, name it in full (`ssh myhost .archipel/bin/archipel connect --stdio`).
- **An app must not be asked a question.** Give SSH `-o BatchMode=yes`, so that a machine whose key is unknown or has changed is refused instead of waiting for an answer nobody sees.
- **Only the first connection needs to pay for the handshake.** With `-o ControlMaster=auto -o ControlPersist=10m -o ControlPath=~/.ssh/app-%C`, SSH keeps one connection to the machine and every later call goes through it. Measured on a local network: 0.19 s for a new connection, 0.01 s through a shared one.
- **A connection that drops** (a closed laptop, another network) ends the command. The SDKs then open a new one and continue from the last item they had.

Checked against the daemon of another machine (Linux, on the local network, OpenSSH), with the command-line client and with the TypeScript and Swift SDKs. The Swift one is a test that runs when told where: `ARCHIPEL_TEST_VIA="ssh myhost archipel connect --stdio" swift test`.

## 2. A WebSocket, on the same machine

```
archipeld --ws 127.0.0.1:7890
```

For what cannot open the local socket or run a command: a page in a browser. **The WebSocket is not encrypted**, so the daemon only accepts a loopback address for it, and every client presents a token.

- **Pairing (preferred):** `archipeld pair --scope interact --name phone` prints a one-time code such as `K7QMP-2XWRA`, valid ten minutes. The app exchanges it for a token of its own: `archipel pair ws://127.0.0.1:7890 CODE --save ~/.archipel-token`, or `pair(url, code)` in the SDKs. The code then stops working.
- **By hand:** `archipeld token add NAME --scope observe` prints a token to copy.
- `archipeld token list` and `archipeld token remove NAME` list and revoke. Both take effect at once.
- The daemon stores only a hash of each token. `ws-token` in its home is the owner's own token, for the command-line client on the same machine.

Scopes: `observe` (list and read), `interact` (also prompt, answer, create), `extend` (also act as an extension), `admin`.

The token matters even on loopback: every program and every user of the machine can connect to a local port.

**From another machine, the WebSocket travels inside SSH** like everything else: `ssh -N -L 7890:127.0.0.1:7890 myhost` makes the remote port answer on your own machine, and the client connects to `ws://127.0.0.1:7890` there. Described, not run here.

**`--ws-exposed`** lets `--ws` take an address other machines can reach. It exists for one case: something in front of the daemon encrypts the traffic and decides who gets in (a reverse proxy with a certificate, or `tailscale serve`, which gives the machine an HTTPS address inside your tailnet; clients then use `wss://…`). Without that, anyone on the network can read the sessions and take the tokens. Not tested here: a `wss://` address end to end.

## 3. A page in a browser

A browser page may connect only from an origin you name:

```
archipeld --ws 127.0.0.1:7890 --ws-origin http://localhost:5173
```

A page cannot set the header the token normally travels in, so it sends it as a WebSocket subprotocol: `webSocketTransport(url, { token, subprotocol: true })`. Any other origin is refused, token or not, because any page your browser shows can try to connect to a local port.

`ui/react/demo/live.tsx` is such a page (`pnpm demo:live` in `ui/react`).

## What is not built

- **Encryption in the daemon** (TLS, or a protocol of its own), and a relay. SSH covers remote access with maintained code; `docs/research/40-ssh-is-the-way-in.md` has the reasoning.
- **Keys that may only reach the daemon, or only read.** Anyone who gets in through SSH has full control, as on the local socket. A daemon is one user's: it is not meant to be shared, and another user runs their own.
- **Protection from other programs running as you** on the daemon's machine: they can open the socket, and read the session log.
- Windows: the local socket is a Unix socket.
