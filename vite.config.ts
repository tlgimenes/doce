import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";

// Injected into the bundle so the running app can report exactly which build
// it is (the Settings footer). The version is the package version; the commit
// pins the exact build (dev or a released .dmg), and falls back to "unknown"
// when git isn't available (e.g. a source-tarball build).
const APP_VERSION = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf-8"),
).version as string;

let GIT_COMMIT = "unknown";
try {
  GIT_COMMIT = execSync("git rev-parse --short HEAD", { encoding: "utf-8" }).trim();
} catch {
  // No git checkout — leave "unknown".
}

// React Compiler requires Babel, which @vitejs/plugin-react v6 no longer
// bundles (it moved to an oxc-based transform for speed). The babel plugin
// must run before the react plugin so the compiler sees un-transformed JSX.
export default defineConfig(async () => ({
  plugins: [await babel({ presets: [reactCompilerPreset()] }), react(), tailwindcss()],
  define: {
    __APP_VERSION__: JSON.stringify(APP_VERSION),
    __GIT_COMMIT__: JSON.stringify(GIT_COMMIT),
  },
  resolve: {
    // Vite doesn't read tsconfig.json's `paths` at runtime — this alias
    // has to be declared separately to match.
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // Debug Tauri builds always navigate to `build.devUrl` (tauri.conf.json),
  // regardless of how the binary was launched — `vite preview` (serving the
  // production `dist/` build) needs to answer on that same port for e2e
  // runs against a `cargo build` binary, which has no dev server behind it.
  preview: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    exclude: ["**/node_modules/**", "**/tests/e2e/**"],
  },
}));
