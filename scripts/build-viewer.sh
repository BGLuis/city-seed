#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p city-ffi --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/city_ffi.wasm viewer/city.wasm
