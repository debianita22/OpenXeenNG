#!/bin/sh
# Builds the browser version into dist/web (serve it with any static server).
#
#   scripts/build-web.sh            release build
#   python3 -m http.server -d dist/web 8000
#
# Needs the wasm32-unknown-unknown target (rustup target add wasm32-unknown-unknown).
# When rustup cannot download it but the standard library sources are
# installed, the script falls back to rebuilding std (-Zbuild-std).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
TARGET=wasm32-unknown-unknown
if rustc --print target-libdir --target "$TARGET" >/dev/null 2>&1 &&
    [ -d "$(rustc --print target-libdir --target "$TARGET")" ]; then
    cargo build --release -p openxeenng-web --target "$TARGET"
else
    echo "note: $TARGET std not installed; rebuilding std from rust-src" >&2
    RUSTC_BOOTSTRAP=1 cargo build --release -p openxeenng-web --target "$TARGET" \
        -Zbuild-std=std,panic_abort
fi
OUT="$ROOT/dist/web"
mkdir -p "$OUT"
cp "target/$TARGET/release/openxeenng_web.wasm" "$OUT/openxeenng.wasm"
cp crates/web/www/index.html crates/web/www/main.js "$OUT/"
ls -l "$OUT"
