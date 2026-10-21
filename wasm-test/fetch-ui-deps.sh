#!/bin/sh
# Downloads the sidebar's UI runtime into wasm-test/vendor/ (gitignored).
#
# The bench page builds its control panel with React + Ant Design, but the
# page has no build step, so the UMD bundles are loaded straight from disk.
# index.html falls back to jsDelivr when a file is missing, so the page also
# works before this script has ever been run.
#
#   usage:  sh wasm-test/fetch-ui-deps.sh
set -eu

CDN=https://cdn.jsdelivr.net/npm
VERSIONS="react-18.3.1 react-dom-18.3.1 dayjs-1.11.18 antd-5.29.3"

OUT="$(cd "$(dirname "$0")" && pwd)/vendor"
mkdir -p "$OUT"

get() { get_url="$1"; get_out="$2";
  curl -sfL --proto '=https' -o "$OUT/$get_out" "$get_url" && echo "  $get_out"
}

echo "vendoring UI dependencies (pinned: $VERSIONS) into $OUT"
get "$CDN/react@18.3.1/umd/react.production.min.js"           react.production.min.js
get "$CDN/react-dom@18.3.1/umd/react-dom.production.min.js"   react-dom.production.min.js
get "$CDN/dayjs@1.11.18/dayjs.min.js"                        dayjs.min.js
get "$CDN/antd@5.29.3/dist/antd.min.js"                       antd.min.js
get "$CDN/antd@5.29.3/dist/reset.css"                         antd-reset.css
get "$CDN/antd@5.29.3/dist/antd.min.js.LICENSE.txt"           antd.min.js.LICENSE.txt
echo "done."
