#!/usr/bin/env bash
# Build the hashes binary natively and for wasm32-wasip1, run both, and compare.
# Needs: rustup target add wasm32-wasip1, and node (22 has WASI built in).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --bin hashes -q
cargo build --release --bin hashes --target wasm32-wasip1 -q
./target/release/hashes > target/hashes-native.txt
node --no-warnings tools/wasi.mjs target/wasm32-wasip1/release/hashes.wasm > target/hashes-wasm.txt
cat target/hashes-wasm.txt
diff target/hashes-native.txt target/hashes-wasm.txt && echo "native and wasm32 hashes identical"
