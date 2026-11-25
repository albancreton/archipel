# Reaching a daemon from another machine

The daemon can run commands as you. So by default it is reachable only through a local socket that only you can open. There are three ways to open it further; pick the first that fits.

## 1. Through SSH

Nothing to configure on the daemon. If you can SSH to the machine, you can use its daemon:

```
archipel --via "ssh myhost archipel connect --stdio" sessions
```

`archipel connect --stdio` runs on the remote machine and carries one connection to the local socket on its stdin and stdout. SSH does the encryption and decides who gets in. The client SDKs have the same: `commandTransport("ssh", ["myhost", "archipel", "connect", "--stdio"])` in TypeScript.

Checked with a local command standing in for SSH. A real SSH hop was not tested here (this machine accepts no key for itself).

## 2. A WebSocket inside a private network

```
archipeld --ws 127.0.0.1:7890
```

Every client presents a token.

- **Pairing (preferred):** `archipeld pair --scope interact --name phone` prints a one-time code such as `K7QMP-2XWRA`, valid ten minutes. The app exchanges it for a token of its own: `archipel pair ws://host:7890 CODE --save ~/.archipel-token`, or `pair(url, code)` in the SDKs. The code then stops working.
- **By hand:** `archipeld token add NAME --scope observe` prints a token to copy.
- `archipeld token list` and `archipeld token remove NAME` list and revoke. Both take effect at once.
- The daemon stores only a hash of each token. `ws-token` in its home is the owner's own token, for the command-line client on the same machine.

Scopes: `observe` (list and read), `interact` (also prompt, answer, create), `extend` (also act as an extension), `admin`.

The WebSocket itself is not encrypted. Bind it to loopback and put something in front that is: Tailscale (`tailscale serve` gives the machine an HTTPS address inside your tailnet and forwards to the local port; clients then use `wss://…`), or a reverse proxy with a certificate. Binding it to a public address directly is a mistake.

Not tested here: a `wss://` address end to end. The Rust client is built with the platform's TLS; the TypeScript and Swift clients use their platform's WebSocket, which handles `wss://`.

## 3. A page in a browser

A browser page may connect only from an origin you name:

```
archipeld --ws 127.0.0.1:7890 --ws-origin http://localhost:5173
```

A page cannot set the header the token normally travels in, so it sends it as a WebSocket subprotocol: `webSocketTransport(url, { token, subprotocol: true })`. Any other origin is refused, token or not, because any page your browser shows can try to connect to a local port.

`ui/react/demo/live.tsx` is such a page (`pnpm demo:live` in `ui/react`).

## What is not built

- A relay or end-to-end encryption of its own. SSH and Tailscale cover remote access with maintained code.
- TLS inside the daemon.
- Windows: the local socket is a Unix socket.
