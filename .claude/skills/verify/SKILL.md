---
name: verify
description: Drive the real doce app (Tauri + WebDriver) to verify frontend changes end-to-end — build, launch, spec-drive, screenshot.
---

# Verifying doce changes in the running app

The app is Tauri; the only automatable surface is the e2e WebDriver harness
(`@wdio/tauri-service`, embedded driver). Plain-browser `vite dev` is useless
for verification: Tauri IPC is absent, the app falls back to onboarding.

## Recipe that works

```bash
# 1. Build with the wdio bridge (frontend + cargo, ~1-4 min incremental).
#    Required after ANY frontend change — dist/ from a plain build lacks the bridge.
./tests/e2e/build-for-e2e.sh

# 2. Run one spec (or a temp spec) against the real app:
DOCE_E2E_SKIP_WIPE=1 WDIO_SPECS=./specs/<your>.spec.ts ./tests/e2e/run-e2e.sh
```

- **ALWAYS set `DOCE_E2E_SKIP_WIPE=1`** — the default run DELETES the user's
  real app data (`~/Library/Application Support/app.doce.desktop`) and forces
  a multi-GB model re-download.
- `WDIO_SPECS` (comma-separated) overrides the spec list in
  `tests/e2e/wdio.conf.ts`.
- Temp verification specs can live in `tests/e2e/specs/` uncommitted; they are
  NOT typechecked by `tsc -b` (tsconfig includes `src` only). Delete after.
- `tests/e2e/specs/helpers.ts` → `startWorkspaceConversationViaComposer(dir,
  taskText)` gets you a real conversation with a real model turn.
- Screenshots: `browser.saveScreenshot(path)` at key points — the one reliable
  evidence channel.

## Gotchas

- **Markdown collapses single newlines**: "one item per line" replies render
  as one wrapped paragraph — do NOT rely on model verbosity for scroll
  overflow. Force it with `browser.setWindowSize(900, 420)` instead.
- The tauri-service invoke bridge ("Tauri core.invoke not available after 5s")
  warns nonfatally but occasionally wedges a whole run (element timeouts right
  after launch). A retry usually clears it.
- A killed run can leave an orphaned `doce` binary holding the single-instance
  lock — later launches silently forward to the wedged instance. Check
  `pgrep -fl doce` before every run; kill only PIDs you can attribute to the
  harness.
- Real model turns take 30s–4min each; `waitforTimeout` is 20s, so wrap
  turn-waits in explicit `waitUntil(..., { timeout: 240000 })` on the
  composer's `contenteditable` flipping back to `"true"`.
- No-wipe runs leave test conversations in the user's real sidebar — mention
  it in your report.
- Skip-wipe assumes a model is already installed; a wiped/fresh machine needs
  the onboarding spec first (it waits out the real download).
