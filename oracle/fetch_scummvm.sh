#!/bin/sh
# Fetches the pinned ScummVM sources used as the reference implementation
# ("oracle") for differential tests and as real-format sample data.
#
# Only the pinned commit, without history, and only the paths the oracle
# reads (sparse checkout, blobs on demand): about 4 MB in a few seconds,
# instead of the commits and trees of the whole repository. A directory that
# is already at the pinned commit is reused; anything else in its place (a
# directory a CI cache restored without its repository, an interrupted
# fetch, another revision) is fetched again from scratch.
set -eu
REV="${SCUMMVM_REV:-caf036c192a1379fa7bbf97046c9d61fd614f8f2}"
DEST="${1:-target/scummvm-ref}"
if [ "$(git -C "$DEST" rev-parse -q --verify HEAD 2>/dev/null || true)" = "$REV" ] &&
    [ -f "$DEST/engines/mm/shared/xeen/sprites.cpp" ]; then
    echo "ScummVM reference at $(git -C "$DEST" rev-parse --short HEAD) in $DEST"
    exit 0
fi
rm -rf "$DEST"
git init --quiet "$DEST"
cd "$DEST"
git remote add origin https://github.com/scummvm/scummvm.git
git config remote.origin.promisor true
git config remote.origin.partialclonefilter blob:none
git sparse-checkout set --no-cone \
    /engines/mm/shared/xeen/ \
    /devtools/create_mm/files/xeen/ \
    /common/random.cpp
git fetch --quiet --depth 1 --filter=blob:none origin "$REV"
git checkout --quiet FETCH_HEAD
echo "ScummVM reference at $(git rev-parse --short HEAD) in $DEST"
