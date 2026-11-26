# SSH is the way in

2026-11-26. A spike on the security of what travels between an app and the daemon. The user asked how an app can count on that traffic being private on the network, what "ironclad" would take while staying optional, and whether a daemon can identify a client. It ended in a decision, taken by the user: **SSH is the remote transport, and the daemon opens nothing on the network.**

## What was there

| Path | Encrypted | The daemon proves itself | The client is identified |
|---|---|---|---|
| Local socket (mode 0600, in a folder of mode 0700) | Never on the network | By the file's path | No: any program running as the user has full control |
| `archipel connect --stdio` through SSH | By SSH | By SSH's host key | By SSH's key; the daemon sees the local socket |
| `--ws` | **No** | **No** | A token gives a scope; its name does not reach the server |

On any address but loopback, the WebSocket let anyone on the path read every message, take the token from the `Authorization` header (the owner's token runs commands as the user), and take a pairing code and the token it is exchanged for. `docs/remote.md` said to bind it to loopback and put Tailscale or a proxy in front; nothing enforced it.

## The two ways considered

**1. Encryption in the daemon.** TLS with a key the daemon makes for itself, the certificate's fingerprint handed over at pairing and pinned by the client, a rate limit and an expiry on tokens, later a key per client instead of a token. It works for every client that can pin a certificate. It costs: a pairing link or QR code (a typed ten-character code cannot hold a fingerprint, short of a password-authenticated key exchange), pinning in each SDK (easy in Swift, awkward with Node's own WebSocket, unknown in React Native), and it leaves browsers out, which cannot pin. A protocol of the daemon's own inside the WebSocket (Noise) would be cryptography to maintain by hand in three SDKs.

**2. SSH only.** The user's observation: the daemon is a window onto harnesses that SSH normally fences in, so the window should have the same fence. Without `--ws` that is already true: the daemon is as reachable as the user's shell, no more. An attacker without SSH access cannot reach it, and one with SSH access could run the harness directly anyway. Only `--ws` was a second door with credentials of its own.

The second was chosen. It removes a credential system from the remote path instead of adding cryptography to it, and it gives a rule that fits in a sentence. Minitel (the user's app, on SSH and tmux) already does all of it on iOS with an SSH library (Citadel, on swift-nio-ssh): a key in the Keychain, host keys trusted on first use, commands run on the machine. That was the part I had flagged as uncertain. Speed is not a reason against it: 0.19 s for a handshake on the local network, 0.01 s through a shared connection, nothing per message.

## What was changed

| Change | Checked by |
|---|---|
| `--ws` refuses an address that is not loopback, before the daemon listens anywhere. `--ws-exposed` lifts this, for a daemon behind something that encrypts | A test of the rule, and one of the daemon program refusing `0.0.0.0` |
| Swift SDK: `StreamTransport`, a connection on bytes the app moves itself (an SSH channel of its own library), and `CommandTransport` on macOS, a connection on a command's input and output | A test with bytes cut anywhere; the live test, with a local command standing in for SSH; a real SSH hop (below) |
| `docs/remote.md` says SSH first, and the WebSocket for the same machine | |

**The real SSH hop**, which milestone 7 had left out: the daemon of a Linux machine on the local network was reached from this Mac with `archipel --via`, with the TypeScript SDK's `commandTransport` and with the Swift SDK's `CommandTransport`, each listing its sessions. Only reads. The Swift check stays as a test that runs when `ARCHIPEL_TEST_VIA` names the command.

M2 (the prototype app) already reached its hosts this way. It now shares one SSH connection per host between its calls.

## Decisions

**The guard is a refusal, not a warning.** A daemon started by a service manager prints its warnings where nobody reads them.

**`--ws-exposed` exists.** A daemon behind `tailscale serve` or a reverse proxy on another machine is a legitimate setup, and the WebSocket is still what a browser needs. The flag's name says what it does, not that it is safe.

**The test does not bind a reachable address.** It would open a port to the network for a moment on every run, and macOS may ask the user about it. The rule is a function, tested as one.

**No SSH library in the Swift SDK.** `Transport` was already something an app can implement. `StreamTransport` does the part every such app would write again (lines from bytes cut anywhere), and the app keeps its choice of library, its keys and its host-key policy.

**Pairing and tokens stay**, for the WebSocket on the same machine and behind a proxy.

## Not done

- **A scope and a name on `archipel connect --stdio`**, for an `authorized_keys` line that gives a key the daemon only, or reading only (`restrict,command="archipel connect --stdio --scope observe --name phone"`). Today a connection through SSH has full control and no name. Not needed while every device is the user's own; needed before a key is handed to anyone else.
- **Who is attached.** The server knows a connection's scope, not which device it is: a token's name is dropped before `connect_as`.
- **`StreamTransport` on an SSH library's channel.** It was tested on bytes and through `CommandTransport`, not with Citadel on a phone.
- **The WebSocket through `ssh -L`**, and `wss://` behind a proxy: described in the guide, not run.
- **A rate limit on failed handshakes, and tokens that expire.** They matter less with the port on loopback.
- **Other programs running as the user** on the daemon's machine can open the socket, and read the log and the harness's own transcripts. Closing that takes sandboxing and encryption at rest, which is another project.
