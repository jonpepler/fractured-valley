#!/usr/bin/env bash
# Build the hash check natively and for wasm32-wasip1, run both, and compare the hashes.
# Usage: wasm/check.sh [N] [M] [T]
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --bin hashcheck -q
cargo build --release --target wasm32-wasip1 --bin hashcheck -q
native=$(./target/release/hashcheck "$@" | tee /dev/stderr | awk '{print $1, $9}')
wasm=$(node --no-warnings wasm/run.mjs target/wasm32-wasip1/release/hashcheck.wasm "$@" | tee /dev/stderr | awk '{print $1, $9}')
if [ "$native" = "$wasm" ]; then echo "native and wasm hashes match"; else echo "MISMATCH"; exit 1; fi
