#!/usr/bin/env bash
# Build a self-contained llama-server for macOS arm64 to bundle as a doce sidecar.
#
# Produces ONE executable (all ggml/llama libs static-linked) whose only dynamic
# deps are always-present system frameworks — no @rpath dylibs to juggle. Metal
# shaders are embedded (no default.metallib at runtime). Deployment target is set
# explicitly (the official prebuilt requires macOS 26; doce's floor is 13.0).
#
# Pin is deliberate: LLAMA_TAG >= b8020 is required for Qwen3.5 (Gated DeltaNet);
# b9993 was validated end-to-end by the Phase-0 coherence spike.
set -euo pipefail

LLAMA_TAG="b9993"
TRIPLE="aarch64-apple-darwin"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="${TMPDIR:-/tmp}/doce-llama-build"
DEST="$ROOT/src-tauri/binaries/llama-server-$TRIPLE"

echo "== clone llama.cpp $LLAMA_TAG (shallow) =="
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
git clone --depth 1 --branch "$LLAMA_TAG" https://github.com/ggml-org/llama.cpp "$BUILD_DIR/llama.cpp"

echo "== configure (static, Metal embedded, no curl/openssl/UI, target 13.0) =="
cmake -B "$BUILD_DIR/build" -S "$BUILD_DIR/llama.cpp" \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF \
  -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON \
  -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF \
  -DLLAMA_BUILD_SERVER=ON -DLLAMA_USE_PREBUILT_UI=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF \
  -DCMAKE_OSX_DEPLOYMENT_TARGET=13.0

echo "== build llama-server =="
cmake --build "$BUILD_DIR/build" -j --config Release --target llama-server

mkdir -p "$ROOT/src-tauri/binaries"
cp "$BUILD_DIR/build/bin/llama-server" "$DEST"

echo "== verify: only system dylibs, deployment target 13.0 =="
otool -L "$DEST"
otool -l "$DEST" | grep -A3 LC_BUILD_VERSION | grep -i minos || true
echo "BUILT: $DEST"
