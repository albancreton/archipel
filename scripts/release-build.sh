#!/bin/sh
# Builds what a release carries: `archipeld` and `archipel` for each kind of machine, one
# archive per kind, and their checksums.
#
#   scripts/release-build.sh [OUT]            (default: target/release-assets)
#
# OUT gets archipel-<target>.tar.gz for each target below, and SHA256SUMS. An archive holds the
# two programs and nothing else. The Linux ones are static (musl, OpenSSL built in), so they
# run whatever the machine has.
#
# Runs on an Apple-silicon Mac, with zig and cargo-zigbuild for the Linux targets
# (brew install zig; cargo install --locked cargo-zigbuild). The Rust targets are added here.
# scripts/release.sh runs this and publishes the result.
set -eu
cd "$(dirname "$0")/.."
OUT="${1:-target/release-assets}"
LINUX="x86_64-unknown-linux-musl aarch64-unknown-linux-musl"
MAC="aarch64-apple-darwin x86_64-apple-darwin"

rustup target add $LINUX $MAC >/dev/null
rm -rf "$OUT" && mkdir -p "$OUT"
for target in $MAC; do
  echo "== $target"
  cargo build --release --locked --target "$target" -p archipeld -p archipel-client
done
for target in $LINUX; do
  echo "== $target"
  cargo zigbuild --release --locked --target "$target" -p archipeld -p archipel-client \
    --features archipel-transport/vendored-tls
done
for target in $MAC $LINUX; do
  tar -czf "$OUT/archipel-$target.tar.gz" -C "target/$target/release" archipeld archipel
done
(cd "$OUT" && shasum -a 256 archipel-*.tar.gz > SHA256SUMS && cat SHA256SUMS)
