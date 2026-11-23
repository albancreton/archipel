# Milestone 7: remote access

_2026-11-22. Checked by the test suites (44 Rust, 77 TypeScript and React, 6 Swift) and a live run of a browser page._

The user guide for this is `docs/remote.md`. This note is what was built and how it was checked.

## What was added

| Addition | Checked by |
|---|---|
| `bridge connect --stdio` and `--via COMMAND`: a connection carried on a command's stdin and stdout, for SSH | Rust and TypeScript tests, with a local command standing in for SSH |
| `bridged pair`: a one-time code an app exchanges for its own token | A test against the daemon program: wrong code refused, the code works once, lower case without the dash works, a code expires; also from TypeScript and Swift |
| Tokens stored as SHA-256 hashes | Same test: the token file does not contain the token |
| Revoking a token while the daemon runs | Same test |
| `--ws-origin`: named web origins may connect, with the token as a WebSocket subprotocol | Transport test; live run in a headless browser |
| TLS in the Rust client, for `wss://` | Built only |
| `pair` and a command transport in the TypeScript SDK, `pair` in the Swift SDK | Their live tests |

## The live browser run

Daemon with the fake agent, a WebSocket, and `--ws-origin http://localhost:5173`. The React demo page served on that origin. A headless Chromium driven through its debugging port.

1. A session was created from the command line and left at its approval.
2. The page connected with the token as a subprotocol, listed the session, and drew it with the approval's three buttons (`requires action`).
3. "No" was clicked in the page. The page then showed the tool call as failed, the approval answered "No", and the agent's reply.
4. `bridge show` on the command line showed the same.
5. The same page served from another port (`:5174`), with the right token: refused.

`img/18-browser-live.png` is the page after step 3.

## Decisions

**Pairing is handled in the daemon program, not the server library.** A connection that presents a code is given to a small handler that accepts one request and closes. The library knows scopes; it does not know how tokens are made.

**A code is 10 characters from 32** (no look-alikes), single use, ten minutes by default: 50 bits against a guesser who needs a WebSocket handshake per try.

**Hashes, not tokens, on disk.** Tokens are 256 random bits, so a plain SHA-256 is enough; there is nothing to slow-hash. The owner's `ws-token` stays readable, for the local command-line client: whoever can read it can already open the local socket.

**The browser path is opt-in per origin.** No wildcard.

**Platform TLS in the Rust client** (`native-tls`), as the design asked: Secure Transport on macOS, SChannel on Windows, OpenSSL on Linux.

## Not done or not verified

- **A real SSH hop.** This machine accepts no key for itself, and I did not change that.
- **`wss://` end to end**, and Tailscale. The guide describes the setup; nothing here ran it.
- **Pairing from a phone**: no QR code, no discovery. The code is typed.
- **Token expiry and rotation.** Tokens live until removed.
- **A rate limit on failed handshakes.**
- **Windows.**
