#!/usr/bin/env bash
# Build the PWA into web/pkg: compile the merge library to WebAssembly and
# generate the JS glue. Requires the wasm32 target and a wasm-bindgen-cli
# matching the wasm-bindgen version in Cargo.lock:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <version from Cargo.lock>
set -euo pipefail
cd "$(dirname "$0")/.."

want=$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, "", $0); print; exit}' Cargo.lock)
have=$(wasm-bindgen --version | awk '{print $2}')
if [[ "$have" != "$want" ]]; then
  echo "wasm-bindgen-cli is $have but Cargo.lock needs $want:" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --force" >&2
  exit 1
fi

cargo rustc --release --lib --target wasm32-unknown-unknown --crate-type cdylib
rm -rf web/pkg
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/pdf_merger.wasm
echo "Built web/pkg. Serve the web/ directory, e.g.: python3 -m http.server -d web 8000"
