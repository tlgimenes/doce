import { expect } from "@wdio/globals";

// Covers quickstart.md §1 (Zero-config first run): the app launches
// straight into hardware detection and an automatic model download, with
// no picker, API key entry, or account step. wdio.conf.ts wipes the app's
// data directory before this spec runs, so it always exercises a genuine
// first run regardless of what was previously installed on the test
// machine. The final test waits out the real, full model download (a few
// GB over the network) rather than just checking that it started — this is
// deliberate: chat.spec.ts runs next in the same suite and depends on a
// real, active model being installed by the time this spec finishes.
//
// EARLY_UI_TIMEOUT: on GitHub Actions' macOS runners specifically (not
// reproducible on real local hardware — confirmed by running this exact
// spec locally under an identical full data wipe, all 4 tests passing in
// ~6.5 minutes), these checks fail even at 60s. A diagnostic pass (see
// git history around 2026-07-04 for the temporary instrumentation)
// narrowed this down further: the webview does navigate to the right URL
// and does run JS (Tiptap's dynamic style injection appears in <head>),
// and React does mount something into #root (childElementCount: 1) — but
// that mounted content's body.innerHTML comes back functionally empty.
// That's consistent with some async initialization the app performs on
// mount (e.g. its initial hardware-detection/settings IPC call) hanging
// indefinitely in this specific CI environment while rendering nothing
// visible in the meantime — not yet root-caused further; see
// specs/001-doce-v1-core/tasks.md's T095 note. 60s is kept here as a
// reasonable, evidence-based budget for genuinely slow (not broken) CI
// runs; it does not fix the actual hang.
const EARLY_UI_TIMEOUT = 60000;

describe("Onboarding (User Story 1: zero-config first run)", () => {
  it("shows the doce heading with no model picker, API key field, or account step", async () => {
    await browser.pause(1500);
    const heading = await browser.$("h1");
    try {
      await heading.waitForExist({ timeout: EARLY_UI_TIMEOUT });
    } catch (err) {
      // Temporary diagnostic (not permanent test logic): App.tsx renders
      // literally nothing while its first invoke() call (listModels()) is
      // pending — which was otherwise invisible to any query. A new,
      // equally temporary [data-testid="ipc-diagnostic"] element in
      // App.tsx now surfaces that promise's real outcome (still pending /
      // resolved with a value / rejected with an error) directly in the
      // DOM, so this reads it instead of just re-throwing blind.
      const diag = await browser.$("[data-testid='ipc-diagnostic']");
      const diagText = await diag
        .getText()
        .catch((e) => `<ipc-diagnostic not found or getText failed: ${e}>`);
      console.log(`[diagnostic] ipc-diagnostic element text: ${diagText}`);
      throw err;
    }
    await expect(heading).toHaveText("doce");

    const apiKeyInputs = await browser.$$("input[type='password']");
    expect(apiKeyInputs.length).toBe(0);
  });

  it("displays real detected hardware info, not a placeholder", async () => {
    const hardwareLine = await browser.$("p*=tier");
    await hardwareLine.waitForExist({ timeout: EARLY_UI_TIMEOUT });
    const text = await hardwareLine.getText();

    // Real chip/RAM values from sysctlbyname, not "unknown"/0 fallbacks.
    expect(text).not.toContain("unknown");
    expect(text).toMatch(/Apple/);
  });

  it("starts downloading the model automatically", async () => {
    const progressLabel = await browser.$("p*=Downloading");
    await progressLabel.waitForExist({ timeout: EARLY_UI_TIMEOUT });
  });

  it("finishes the download, verifies it, and transitions to the empty-state composer", async () => {
    // 006-chat-empty-state: a fresh install has no conversations yet, so
    // the landing view is the composer, not a chat thread.
    const composerInput = await browser.$("[data-testid='empty-state-input']");
    await composerInput.waitForExist({ timeout: 11 * 60 * 1000, interval: 2000 });
  });
});
