#!/bin/sh
# Fetches the pinned ScummVM sources used as the reference implementation
# ("oracle") for differential tests and as real-format sample data.
set -eu
REV="${SCUMMVM_REV:-caf036c192a1379fa7bbf97046c9d61fd614f8f2}"
DEST="${1:-target/scummvm-ref}"
if [ ! -d "$DEST/.git" ]; then
    git clone --quiet --filter=blob:none --no-checkout \
        https://github.com/scummvm/scummvm.git "$DEST"
fi
cd "$DEST"
git sparse-checkout set --no-cone \
    /engines/mm/shared/xeen/ \
    /devtools/create_mm/files/xeen/ \
    /common/random.cpp
git checkout --quiet "$REV"
echo "ScummVM reference at $(git rev-parse --short HEAD) in $DEST"
