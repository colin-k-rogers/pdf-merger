#!/usr/bin/env bash
# Build static Linux binaries (x86_64 + aarch64, musl) and publish a GitHub release.
# Usage: scripts/release.sh [--dry-run]   (version comes from Cargo.toml)
# Needs: rustup, zig, cargo-zigbuild, gh (authenticated).
#   brew install zig gh && cargo install cargo-zigbuild
set -euo pipefail
cd "$(dirname "$0")/.."

dry_run=0
[[ "${1:-}" == "--dry-run" ]] && dry_run=1

for tool in cargo rustup zig cargo-zigbuild gh; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done

version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
tag="v$version"
targets=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)

if [[ -n "$(git status --porcelain)" ]]; then
  echo "working tree is dirty; commit first" >&2; exit 1
fi
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "tag $tag already exists; bump the version in Cargo.toml" >&2; exit 1
fi

dist=target/dist
rm -rf "$dist" && mkdir -p "$dist"
rustup target add "${targets[@]}"

assets=()
for t in "${targets[@]}"; do
  cargo zigbuild --release --target "$t"
  name="pdf-merger-${t%%-*}-unknown-linux-musl"   # pdf-merger-<arch>-unknown-linux-musl
  stage="$dist/$name"
  mkdir -p "$stage"
  cp "target/$t/release/pdf-merger" README.md "$stage/"
  tar -C "$stage" -czf "$dist/$name.tar.gz" pdf-merger README.md
  assets+=("$dist/$name.tar.gz")
done
(cd "$dist" && shasum -a 256 ./*.tar.gz > SHA256SUMS)
assets+=("$dist/SHA256SUMS")

if (( dry_run )); then
  echo "dry run: built ${assets[*]}; would create $tag"
  exit 0
fi

git tag "$tag"
git push origin "$tag"
gh release create "$tag" "${assets[@]}" --title "$tag" --generate-notes
