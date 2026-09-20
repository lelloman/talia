#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
node dashboard/compile.mjs
node dashboard/compile.mjs dashboard/examples/monitoring.package.json dashboard/generated/monitoring.json
NODE_PATH=spikes/runtime/node_modules spikes/runtime/node_modules/.bin/esbuild dashboard/web/worker.js --bundle --format=esm --outfile=dashboard/web/dist/worker.js
cp spikes/runtime/node_modules/@jitl/quickjs-ng-wasmfile-release-sync/dist/emscripten-module.wasm dashboard/web/dist/ng.wasm
