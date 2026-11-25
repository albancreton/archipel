source ~/.zshrc >/dev/null 2>&1
export PROBE_MODEL="local/Qwen3.8-27B-BF16-DFLASH2"
node m0.mjs "$1" pi "${@:2}" 2>&1
