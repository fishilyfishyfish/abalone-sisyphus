#!/usr/bin/env bash
# 엔진을 WASM으로 빌드해서 web/pkg에 넣는다.
# 필요: rustup target add wasm32-unknown-unknown, cargo install wasm-bindgen-cli (Cargo.lock의 wasm-bindgen과 같은 버전)
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --manifest-path engine/Cargo.toml --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  engine/target/wasm32-unknown-unknown/release/abalone_engine.wasm
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 web/pkg/abalone_engine_bg.wasm -o web/pkg/abalone_engine_bg.wasm
fi
ls -l web/pkg
