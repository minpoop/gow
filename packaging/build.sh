#!/usr/bin/env bash
# Cross-build GowRing.exe + gowring.dll for Windows and zip the release into dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=x86_64-pc-windows-msvc
cargo run -q -p gowring-sheets --bin preflight
cargo test -q --workspace
cargo xwin build -q --release --target $TARGET -p gowring-launcher
(cd crates/gowring-dll && cargo xwin build -q --release --target $TARGET)
python3 -I packaging/package.py
