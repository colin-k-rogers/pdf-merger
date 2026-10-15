#!/usr/bin/env bash
# Build the PWA into web/pkg: compile the merge library to WebAssembly and
# generate the JS glue. Requires the wasm32 target and a wasm-bindgen-cli
# matching the wasm-bindgen version in Cargo.lock:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <version from Cargo.lock>
set -euo pipefail
cd "$(dirname "$0")/.."

cargo rustc --release --lib --target wasm32-unknown-unknown --crate-type cdylib
rm -rf web/pkg
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/pdf_merger.wasm
echo "Built web/pkg. Serve the web/ directory, e.g.: python3 -m http.server -d web 8000"
