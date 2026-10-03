#!/bin/sh
# Builds the differential-test oracle: a small C++ program that runs code
# extracted verbatim from ScummVM's Xeen engine (GPL-3.0-or-later).
#
#   oracle/build.sh            -> target/oracle/xeen_oracle
#   OPENXEENNG_ORACLE=target/oracle/xeen_oracle cargo test -p openxeenng-engine --test oracle
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF="${SCUMMVM_DIR:-$ROOT/target/scummvm-ref}"
OUT="$ROOT/target/oracle"
if [ ! -f "$REF/engines/mm/shared/xeen/sprites.cpp" ]; then
    "$ROOT/oracle/fetch_scummvm.sh" "$REF"
fi
mkdir -p "$OUT"
SPR="$REF/engines/mm/shared/xeen/sprites.cpp"

# #defines at the top of sprites.cpp
awk '/^#define SCREEN_WIDTH 320/{p=1} p&&/^int SpriteResource::_clippedBottom;/{exit} p' \
    "$SPR" > "$OUT/sprites_defs.inc"
# getScaledVal .. SpriteResource::draw(dest, frame, pos, bounds, flags, scale)
# The only edit: ScummVM caches drawer 5's threshold in a function-level
# `static`, freezing the first value; OpenXeenNG implements the evident intent.
awk '/^static inline uint getScaledVal/{p=1} p&&/^void SpriteResource::draw\(XSurface &dest, int frame\) const/{exit} p' \
    "$SPR" | sed 's/static const uint16 threshold = DRAWER5_THRESHOLD/const uint16 threshold = DRAWER5_THRESHOLD/' \
    > "$OUT/sprites_draw.inc"
awk '/^enum \{/{p=1} p&&/^class SpriteResource/{exit} p' \
    "$REF/engines/mm/shared/xeen/sprites.h" > "$OUT/sprites_enums.inc"
awk '/^uint16 BaseCCArchive::convertNameToId/{p=1} p{print} p&&/^}/{exit}' \
    "$REF/engines/mm/shared/xeen/cc_archive.cpp" > "$OUT/cc_hash.inc"
awk '/^void BaseCCArchive::loadIndex/{p=1} p{print} p&&/^}/{exit}' \
    "$REF/engines/mm/shared/xeen/cc_archive.cpp" > "$OUT/cc_index.inc"
awk '/^void RandomSource::setSeed/{p=1} p&&/^} \/\/ End of namespace Common/{exit} p' \
    "$REF/common/random.cpp" > "$OUT/random.inc"

for f in sprites_defs sprites_draw sprites_enums cc_hash cc_index random; do
    if [ ! -s "$OUT/$f.inc" ]; then
        echo "extraction of $f failed (upstream layout changed?)" >&2
        exit 1
    fi
done
"${CXX:-g++}" -std=c++17 -O2 -Wall -Wno-unused-function -I"$OUT" \
    -o "$OUT/xeen_oracle" "$ROOT/oracle/harness.cpp"
echo "built $OUT/xeen_oracle"
