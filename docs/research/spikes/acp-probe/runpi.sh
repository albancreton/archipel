source ~/.zshrc >/dev/null 2>&1
export PROBE_MODEL="local/Qwen3.8-27B-BF16-DFLASH2"
for s in "$@"; do node client.mjs pi $s 2>&1 | cut -c1-520 | grep -v "^  session:"; echo; done
