import { existsSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../..");
const appBinaryPath = path.join(repoRoot, "src-tauri/target/debug/doce");

// Tauri's `identifier` (tauri.conf.json) -> macOS app-support directory.
const appDataDir = path.join(os.homedir(), "Library/Application Support/app.doce.desktop");

export const config: WebdriverIO.Config = {
  runner: "local",
  // Explicit order, not a glob: onboarding must run first (it wipes app
  // data below, then waits out the real model download) so chat.spec.ts
  // can rely on a model already being installed and active. Both specs
  // share one persistent app-data directory across the whole suite, not
  // an isolated one per spec file.
  specs: process.env.WDIO_SPECS ? process.env.WDIO_SPECS.split(",") : ["./specs/onboarding.spec.ts", "./specs/chat.spec.ts"],
  maxInstances: 1,

  onPrepare: () => {
    // DOCE_E2E_SKIP_WIPE: for local iteration against an already-installed
    // model (avoids re-triggering a multi-GB download every run) — CI and
    // the default full-suite run always wipe for a genuine clean slate.
    if (process.env.DOCE_E2E_SKIP_WIPE) return;
    if (existsSync(appDataDir)) {
      rmSync(appDataDir, { recursive: true, force: true });
    }
  },

  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath,
        driverProvider: "embedded",
        captureBackendLogs: true,
        captureFrontendLogs: true,
      },
    ],
  ],

  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: appBinaryPath,
      },
    },
  ],

  logLevel: "info",
  bail: 0,
  waitforTimeout: 20000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,

  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    // A few GB over a real network, plus sha256 verification, plus first
    // model load: budget generously so the harness itself is never the
    // reason a real, working download+load path is reported as a failure.
    timeout: 12 * 60 * 1000,
  },
};
