#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../runtime"
NODE_PATH=./node_modules node_modules/.bin/esbuild ../transport/web/worker.js --bundle --format=esm --outfile=../transport/web/dist/worker.js
