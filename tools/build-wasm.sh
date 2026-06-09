#!/usr/bin/env bash
# Build the browser engine (crates/fpc-wasm) to pkg/ for the UI to import.
#
# Note: this machine has BOTH Homebrew rust (no wasm32 std) and rustup. cargo
# resolves `rustc` from PATH (Homebrew) unless we pin RUSTC to the rustup
# toolchain that actually has the wasm32 target. Hence the RUSTC export below.
#
#   ./tools/build-wasm.sh
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=wasm32-unknown-unknown
WASM=target/$TARGET/release/fpc_wasm.wasm

export RUSTC="$(rustup which --toolchain stable rustc)"
rustup run stable cargo build -p fpc-wasm --release --target "$TARGET"

# JS glue (ES module) into pkg/
wasm-bindgen "$WASM" --out-dir pkg --target web

echo "built pkg/ -> $(ls pkg/)"
