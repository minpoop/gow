#!/usr/bin/env bash
# Live-Kratos stage 1: cross-build GowRProbe.exe + gowr_probe.dll and zip them into dist/GowRProbe-<ver>.zip.
# A separate download from the mod itself (docs/LIVE-KRATOS-PLAN.md); nothing from either game is included.
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=x86_64-pc-windows-msvc
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/gowr-probe/Cargo.toml | head -1)
RUSTFLAGS="-C target-feature=+crt-static" cargo xwin build -q --release --target $TARGET -p gowring-launcher --bin GowRProbe
(cd crates/gowr-probe && RUSTFLAGS="-C target-feature=+crt-static" cargo xwin build -q --release --target $TARGET)
STAGE=$(mktemp -d)/GowRProbe
mkdir -p "$STAGE" dist
cp target/$TARGET/release/GowRProbe.exe "$STAGE/"
cp crates/gowr-probe/target/$TARGET/release/gowr_probe.dll "$STAGE/"
cp packaging/probe-README.txt "$STAGE/README.txt"
cat > "$STAGE/THIRD-PARTY.txt" <<'TXT'
GowRProbe, part of "Kratos in the Lands Between" (unofficial fan project; contains no game files).
Compiled in: windows-rs / windows-sys (Microsoft, MIT or Apache-2.0), retour (BSD-2-Clause), png (MIT or Apache-2.0),
and the Rust standard library (MIT or Apache-2.0).
TXT
rm -f "dist/GowRProbe-$VER.zip"
(cd "$(dirname "$STAGE")" && zip -q -r "$OLDPWD/dist/GowRProbe-$VER.zip" GowRProbe)
ls -l "dist/GowRProbe-$VER.zip"
