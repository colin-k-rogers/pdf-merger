#!/usr/bin/env bash
# Build static Linux binaries (x86_64 + aarch64, musl) plus the packaged web app
# (PWA zip) and publish a GitHub release.
# Usage: scripts/release.sh [--dry-run | --build-only]   (version comes from Cargo.toml)
#   --dry-run     build everything, but don't tag, push or publish
#   --build-only  same, minus the tag checks; used by CI, which publishes the
#                 assets itself (.github/workflows/release.yml)
# Normally you don't run this: bump the version, then push a tag and CI does it.
# Running it without flags also works (CI then just uploads into your release).
# Needs: rustup, zig, cargo-zigbuild, wasm-bindgen-cli (version from Cargo.lock),
# zip, gh (authenticated).
#   brew install zig gh && cargo install cargo-zigbuild
set -euo pipefail
cd "$(dirname "$0")/.."

dry_run=0
build_only=0
case "${1:-}" in
  --dry-run) dry_run=1 ;;
  --build-only) build_only=1 ;;
  "") ;;
  *) echo "usage: $0 [--dry-run | --build-only]" >&2; exit 1 ;;
esac

for tool in cargo rustup zig cargo-zigbuild wasm-bindgen zip gh; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done

version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
tag="v$version"
targets=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)

if [[ -n "$(git status --porcelain)" ]]; then
  echo "working tree is dirty; commit first" >&2; exit 1
fi
if (( build_only )); then
  if [[ "${GITHUB_REF_TYPE:-}" == "tag" && "$GITHUB_REF_NAME" != "$tag" ]]; then
    echo "pushed tag $GITHUB_REF_NAME doesn't match Cargo.toml version ($tag)" >&2; exit 1
  fi
elif git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "tag $tag already exists; bump the version in Cargo.toml" >&2; exit 1
fi

dist=target/dist
rm -rf "$dist" && mkdir -p "$dist"
rustup target add "${targets[@]}" wasm32-unknown-unknown

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
# Web app: static files to host over HTTPS (see README). Built last-to-first so
# a wasm toolchain problem fails before anything is tagged.
scripts/build-web.sh
web_name="pdf-merger-web-$tag"
web_stage="$dist/$web_name"
mkdir -p "$web_stage"
cp -R web/. "$web_stage/"
cp README.md "$web_stage/"
(cd "$dist" && zip -qr "$web_name.zip" "$web_name")
assets+=("$dist/$web_name.zip")

(cd "$dist" && shasum -a 256 ./*.tar.gz ./*.zip > SHA256SUMS)
assets+=("$dist/SHA256SUMS")

if (( build_only )); then
  echo "built ${assets[*]}"
  exit 0
fi
if (( dry_run )); then
  echo "dry run: built ${assets[*]}; would create $tag"
  exit 0
fi

git tag "$tag"
git push origin "$tag"
# Pushing the tag also starts the CI Release workflow, which publishes the same
# release; whichever gets there second uploads into the existing one.
gh release create "$tag" "${assets[@]}" --title "$tag" --generate-notes \
  || gh release upload "$tag" "${assets[@]}" --clobber
