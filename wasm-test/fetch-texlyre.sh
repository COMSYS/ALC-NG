#!/bin/sh
# Downloads the TeXlyre BusyTeX build (TeX Live 2026 as one wasm module) into
# wasm-test/texlyre/, next to the tracked alc-texlyre-worker.js adapter.
#
#   sh wasm-test/fetch-texlyre.sh            # engine + texlive-basic (~130 MB)
#   BIBER=1 sh wasm-test/fetch-texlyre.sh    # also the biber module (~30 MB)
#
# Pinned to one build of https://github.com/TeXlyre/texlyre-busytex-build: its
# JS API and the wasm module change together. Only `texlive-basic` is needed:
# the rest of TeX Live comes from the server's /texlive/ proxy on demand.
set -eu

TAG=build_wasm_46ebb6216a5c1fe63dd1dc885831d1a8cef2dc86_31802832436_1
BASE="https://github.com/TeXlyre/texlyre-busytex-build/releases/download/$TAG"
DIR="$(cd "$(dirname "$0")" && pwd)/texlyre"

FILES="busytex.js busytex.wasm busytex_pipeline.js busytex_biber.js busytex_worker.js texlive-basic.js texlive-basic.data"
if [ "${BIBER:-}" = 1 ]; then FILES="$FILES biber.js biber.wasm biber.data"; fi

mkdir -p "$DIR"
for f in $FILES; do
    if [ -s "$DIR/$f" ]; then echo "have $f"; continue; fi
    echo "fetching $f"
    curl -sSfL -o "$DIR/$f.part" "$BASE/$f"
    mv "$DIR/$f.part" "$DIR/$f"
done
