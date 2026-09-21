#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ! -d dashboard/web/node_modules/vue ]]; then
  echo 'Missing web shell dependencies: run npm ci --prefix dashboard/web' >&2
  exit 1
fi
node dashboard/compile.mjs
node dashboard/compile.mjs dashboard/examples/monitoring.package.json dashboard/generated/monitoring.json
NODE_PATH=spikes/runtime/node_modules spikes/runtime/node_modules/.bin/esbuild dashboard/web/worker.js --bundle --format=esm --outfile=dashboard/web/dist/worker.js
cp spikes/runtime/node_modules/@jitl/quickjs-ng-wasmfile-release-sync/dist/emscripten-module.wasm dashboard/web/dist/ng.wasm

NODE_PATH=dashboard/web/node_modules spikes/runtime/node_modules/.bin/esbuild dashboard/web/chrome.js --bundle --format=esm --minify --define:process.env.NODE_ENV=\"production\" --outfile=dashboard/web/dist/chrome.js
