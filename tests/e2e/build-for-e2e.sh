#!/usr/bin/env bash
# Builds the app for e2e testing: stages the wdio-only capability file
# (kept out of src-tauri/capabilities/ normally — see src-tauri/src/lib.rs
# and tauri.conf.test.json for why), builds the frontend with the wdio
# bridge included, builds the Rust binary with the `wdio` feature, then
# removes the staged capability so a subsequent normal `cargo build` isn't
# affected.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

cleanup() {
  rm -f src-tauri/capabilities/wdio.json
}
trap cleanup EXIT

cp src-tauri/e2e-resources/wdio.capability.json src-tauri/capabilities/wdio.json

VITE_E2E_TESTING=true npm run build

# `cargo build` doesn't understand tauri.conf.json overrides directly — that
# merging is normally done by the `tauri` CLI's own `--config` flag, which
# just sets this same env var for tauri-build's build.rs to pick up.
export TAURI_CONFIG="$(cat src-tauri/tauri.conf.test.json)"
(cd src-tauri && cargo build --features wdio)
