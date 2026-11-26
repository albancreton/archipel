#!/bin/sh
# Publishes a release of the version in Cargo.toml: tags it, builds `archipeld` and `archipel`
# for each kind of machine (scripts/release-build.sh) and uploads them with `gh`.
#
#   scripts/release.sh
#
# Run it on an Apple-silicon Mac, on a clean main that is pushed. It is done by hand and not
# by GitHub Actions because the four builds take a few minutes here and over twenty there.
# Before it: set `version` under [workspace.package] in Cargo.toml and in ui/gpui/Cargo.toml,
# run `UPDATE_VECTORS=1 cargo test -p archipel-server --test daemon`, commit and push.
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
tag="v$version"

[ -z "$(git status --porcelain)" ] || { echo "The working copy has changes; a release is built from a commit."; exit 1; }
git fetch -q origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "HEAD is not origin/main; push first."; exit 1; }
if gh release view "$tag" >/dev/null 2>&1; then echo "$tag is already released."; exit 1; fi

scripts/release-build.sh target/release-assets
git tag "$tag" 2>/dev/null || [ "$(git rev-parse "$tag^{commit}")" = "$(git rev-parse HEAD)" ] || { echo "$tag exists on another commit."; exit 1; }
git push origin "$tag"
gh release create "$tag" target/release-assets/* --title "$tag" --generate-notes
