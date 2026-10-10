#!/usr/bin/env bash
# End-to-end check of GowRing.exe under Wine against fakes (no real game involved):
#   --diagnose, --prepare, --play (fake ModEngine2 launcher + fake eldenring.exe).
# Usage: tools/wine-e2e.sh   (after: cargo xwin build --release --target x86_64-pc-windows-msvc -p gowring-launcher
#                              and: (cd tools/fakes && cargo xwin build --release --target x86_64-pc-windows-msvc))
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SB="${SANDBOX:-/tmp/gowring-e2e}"
WINE="${WINE:-/usr/lib/wine/wine64}"
export WINEPREFIX="${WINEPREFIX:-/home/user/tools/wineprefix}" WINEDEBUG=-all
rm -rf "$SB"; mkdir -p "$SB"/{managed/modengine2,er/Game,local}
cp "$ROOT/target/x86_64-pc-windows-msvc/release/GowRing.exe" "$SB/managed/"
cp "$ROOT/tools/fakes/target/x86_64-pc-windows-msvc/release/modengine2_launcher.exe" "$SB/managed/modengine2/"
cp "$ROOT/tools/fakes/target/x86_64-pc-windows-msvc/release/eldenring.exe" "$SB/er/Game/"
echo "fake dll" > "$SB/managed/gowring.dll"
"$ROOT/target/debug/gow-fixture" "$SB/gow" >/dev/null
W() { printf 'Z:%s' "$(echo "$1" | tr / '\\')"; }
export GOWRING_ER="$(W $SB/er)" GOWRING_GOWR="$(W $SB/gow)" GOWRING_LOCAL="$(W $SB/local)" FAKE_LOG="$(W $SB/fake.log)"
run() { echo "--- $*"; timeout 90 "$WINE" "$SB/managed/GowRing.exe" "$@"; }
run --version
run --diagnose > "$SB/diag.out"; head -8 "$SB/diag.out"; grep -c "match(es)" "$SB/diag.out"
run --prepare
test -f "$SB/managed/.melty-ready" && echo "ready token: $(cat $SB/managed/.melty-ready)"
test -f "$SB/local/cache/hud/health_frame.png" && echo "converted PNG: $(wc -c < $SB/local/cache/hud/health_frame.png) bytes"
python3 -I -c "import json;m=json.load(open('$SB/local/cache/manifest.json'));print('missing:',sorted(k for k,v in m['report']['per_asset'].items() if v['state']!='converted')[:6],'...');print('converted:',sorted(k for k,v in m['report']['per_asset'].items() if v['state']=='converted'))"
run --play
echo "--- fake ModEngine2 saw:"; cat "$SB/fake.log"
