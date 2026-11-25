# record: sessions of the real Claude adapter, for the tests

`v2rec.mjs` drives the Claude adapter over ACP v2 through fixed scenarios and writes every message to `testdata/transcripts/claude-v2/<scenario>.ndjson`. Those recordings are what the Rust core, both client SDKs and both kits are tested with.

```
cd tools/record
npm install                        # the Claude adapter and pi-acp, at the versions the recordings were made with
mkdir -p work                      # the scratch project the sessions run in; add a small package.json and tsconfig.json
node v2rec.mjs <scenario|all>      # a live, paid call per scenario (Haiku)
cd ../.. && scripts/scrub-recordings.py testdata/transcripts && just vectors
```

It records on Haiku. Set `CLAUDE_CODE_EXECUTABLE` to the installed `claude` if the adapter should use that one instead of its own copy.

Always scrub before committing: a recording holds the paths, the account's plan and the commands of the machine it was made on. Then review the diff in `testdata/vectors`.

The conformance tests (`conformance/config.example.json`) use the adapters installed here, so both run the same versions. `tools/watch/sources.json` names those versions as `current`.

This package uses npm and is not part of the pnpm workspace: it only exists to pin and install the adapters.
