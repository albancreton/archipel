# Recent activity in the Claude and Codex providers

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

**The question asked:**

> ignoring Antigravity. What is the recent activity in the claude and codex servers ? Do they get a lot of updates ?
> Trying to figure out why they have not yet need moved to a `packages/provider-*`

---

Yes, they are the two most actively changed providers in the repo, and the repo records no reason for leaving them behind. Their churn and their coupling to the rest of the server are the likely reasons, but that is my inference.

## Activity

Commits touching each provider's code (the repo as a whole had 5,612 commits in the last 30 days):

| | Last 7 days | Last 30 days | Last 90 days |
|---|---|---|---|
| Claude (all `Claude*` files in the server) | 42 | 230 | 439 |
| Codex (all `Codex*` files plus `effect-codex-app-server`) | 30 | 194 | 361 |
| `provider-acp` (shared by five drivers) | 53 | 153 | 196 |
| `provider-opencode` | 18 | 121 | 184 |
| `provider-cursor` | 13 | 82 | 116 |
| `provider-pi` | 18 | 72 | 118 |
| `provider-muse` | 5 | 33 | 62 |

- **Mostly fixes:** over the last 30 days Claude had 183 fixes against 21 features, and Codex 139 against 13.
- **Steady, not a burst:** the Claude adapter file alone has taken 23 to 71 commits a week for the last seven weeks, and the Codex adapter 7 to 40.
- **Still growing:** since the extraction day (2026-10-08) the Claude adapter went from 8,238 to 15,194 lines and the Codex adapter from 7,350 to 9,367.
- **Concentrated ownership:** Julius Marminge authored 165 of the commits touching either provider in the last 30 days; the next contributor has 28.

## Why they may not have moved

The only statement in the repo is one line in `docs/internals/adding-a-provider.md`: "Drivers not moved yet still live in `provider/Drivers`". The extraction commits have no body text, and there is no `provider-claude` or `provider-codex` reference anywhere. What the code does show:

- **Timing.** Both adapters were created on 2026-10-02 with the new orchestrator. The package extraction happened six days later and took the smaller providers.
- **Churn makes a move expensive.** A file taking 30 or more commits a week means a move conflicts with every in-flight branch.
- **Other server code imports them.** `ws.ts`, `server.ts`, `terminal/Manager.ts`, `project/AgentSessionScanner.ts`, the `usage/` readers and `ProviderTurnStartService.ts` all import Claude or Codex modules directly. The packaged providers have no such callers.
- **They import the server.** The Claude adapter has 26 relative imports into `apps/server` and the Codex adapter 20, against the rule that a package reaches the server only through `ProviderHost`.
- **Codex's managed mode is wired into the server binary.** The token helper is a fast path in `binMain.ts`, and the ChatGPT sign-in, token broker and installer are server services.
- **Claude has process-level entry points.** It has its own history worker (`claude-history-worker.ts`) that the single-executable build exposes as a subcommand.

So "not moved yet" reads as "hardest and busiest last" rather than a decision to keep them in core. Julius or Theo would be the ones to confirm whether a move is planned.
