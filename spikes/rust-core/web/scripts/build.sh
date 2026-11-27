#!/usr/bin/env bash
# Builds everything the web harness serves, into web/dist:
#   pkg-st/      the single-threaded WebAssembly build (stable Rust)
#   pkg-mt/      the threaded build (nightly: shared memory, rayon on Web Workers)
#   native.json  the native build's hash list for the determinism scenario
#   native-tree.json  the same for the dead-tree scene (the speed test)
#   js/, *.html  the pages
#
#   bash scripts/build.sh          everything
#   bash scripts/build.sh js       only the pages (after editing TypeScript)
#   SKIP_THREADS=1 bash scripts/build.sh   no nightly build
#   FV_NIGHTLY=nightly-2026-11-25 bash scripts/build.sh   a specific nightly
#
# Needs wasm-bindgen-cli at the same version as the crate (0.2.130):
#   cargo install wasm-bindgen-cli --version 0.2.130 --locked
set -euo pipefail
cd "$(dirname "$0")/.."
WEB=$PWD
CORE=$WEB/..                                  # spikes/rust-core (the workspace)
TARGET=${CARGO_TARGET_DIR:-$CORE/target}
mkdir -p dist

if [ "${1:-all}" != js ]; then
  echo "== native hash list"
  (cd "$CORE" && CARGO_TARGET_DIR=$TARGET cargo run --quiet --release -p fv-wasm --example native_hashes) > dist/native.json
  (cd "$CORE" && CARGO_TARGET_DIR=$TARGET cargo run --quiet --release -p fv-wasm --example native_hashes -- tree) > dist/native-tree.json

  echo "== single-threaded wasm (stable)"
  (cd "$CORE" && CARGO_TARGET_DIR=$TARGET cargo build --release -p fv-wasm --target wasm32-unknown-unknown)
  wasm-bindgen --target web --out-dir dist/pkg-st "$TARGET/wasm32-unknown-unknown/release/fv_wasm.wasm"

  if [ -z "${SKIP_THREADS:-}" ]; then
    echo "== threaded wasm (nightly)"
    # Atomics + shared memory, and std rebuilt with them (the shipped std is
    # built without). The --export flags let wasm-bindgen set up each
    # worker's thread-local storage. Kept in a separate target dir so these
    # flags never mix with the stable build's.
    (cd "$CORE" && CARGO_TARGET_DIR=$TARGET/threads \
      RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--max-memory=1073741824 -C link-arg=--import-memory -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base' \
      cargo +"${FV_NIGHTLY:-nightly}" build --release -p fv-wasm --features threads --target wasm32-unknown-unknown -Z build-std=panic_abort,std)
    wasm-bindgen --target web --out-dir dist/pkg-mt "$TARGET/threads/wasm32-unknown-unknown/release/fv_wasm.wasm"
  fi
fi

echo "== pages"
cp public/* dist/
npx esbuild src/determinism.ts src/threads.ts src/threads-worker.ts src/peers.ts src/speed.ts src/speed-worker.ts \
  --bundle --format=esm --splitting --target=es2022 --outdir=dist/js --log-level=warning
echo "built $WEB/dist"
