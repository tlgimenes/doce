# Bundled llama-server

doce ships `llama-server` (llama.cpp's OpenAI-compatible inference server) as a
Tauri sidecar and drives all generation over its HTTP API. The binary is built
from source, not taken from the official releases, for two reasons:

1. **macOS floor.** Official `macos-arm64` release binaries are built with a
   minimum deployment target of macOS 26 and refuse to launch on older systems.
   doce supports macOS 13+, so we build with `CMAKE_OSX_DEPLOYMENT_TARGET=13.0`.
2. **Single self-contained file.** Releases are dynamically linked against ~10
   of their own dylibs. `-DBUILD_SHARED_LIBS=OFF` links everything into one
   executable whose only remaining dynamic deps are always-present system
   frameworks (Metal, Foundation, libc++, libSystem). Metal shaders are embedded
   (`GGML_METAL_EMBED_LIBRARY=ON`), so no `default.metallib` is needed at runtime.

## Build

```
./scripts/build-llama-server.sh
```

Output: `src-tauri/binaries/llama-server-aarch64-apple-darwin` (git-ignored;
Tauri resolves it as the `llama-server` sidecar via `bundle.externalBin`).

## Pin

`LLAMA_TAG` in the script. Must be **≥ b8020** (Qwen3.5 Gated DeltaNet support,
merged early Feb 2026). Currently `b9993`, validated end-to-end by the Phase-0
coherence spike on Apple M1 Pro. Bumping the pin is deliberate — re-verify
Qwen3.5 tool-call output after any change.
