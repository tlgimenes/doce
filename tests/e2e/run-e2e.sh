#!/usr/bin/env bash
# Runs the e2e suite. A debug Tauri build always navigates its window to
# `build.devUrl` (tauri.conf.json), regardless of how the binary was
# launched — there's no way to make a `cargo build`-produced binary load
# `frontendDist` instead short of a release build. So this serves the
# already-built `dist/` via `vite preview` on that same port before handing
# off to wdio, and tears the preview server down afterward either way.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

npx vite preview --port 1420 --strictPort > /tmp/doce-e2e-preview.log 2>&1 &
PREVIEW_PID=$!

cleanup() {
  kill "$PREVIEW_PID" 2>/dev/null
}
trap cleanup EXIT

for _ in $(seq 1 20); do
  if curl -sf http://localhost:1420 > /dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

npx wdio run ./tests/e2e/wdio.conf.ts
STATUS=$?

exit $STATUS
