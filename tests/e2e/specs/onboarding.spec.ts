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
// reproducible on real local hardware, confirmed by running this exact
// spec locally under an identical full data wipe — all 4 tests passed in
// ~6.5 minutes), these first few UI checks were seen timing out at their
// previous, tighter values (15-20s). CI's runner reports its GPU as "MTL0
// (Apple Paravirtual device)" (captured from a real run's backend log) —
// a paravirtualized, not passed-through, Metal device — and one
// unrelated but revealing data point from that same run: the Metal
// shader library alone took 11+ seconds to load, something that's near-
// instant on real hardware. That's strong evidence this environment runs
// at a fraction of real-hardware speed for anything graphics/webview-
// related, so these checks get a much more generous budget here than a
// real machine would ever need.
const EARLY_UI_TIMEOUT = 60000;

describe("Onboarding (User Story 1: zero-config first run)", () => {
  it("shows the Doce heading with no model picker, API key field, or account step", async () => {
    await browser.pause(1500);
    const heading = await browser.$("h1");
    try {
      await heading.waitForExist({ timeout: EARLY_UI_TIMEOUT });
    } catch (err) {
      // Temporary diagnostic (not permanent test logic): investigating why
      // the webview renders nothing at all specifically in GitHub Actions
      // CI. Dump what the webview actually has, if anything, to distinguish
      // "never navigated" (blank/about:blank) from "navigated but React
      // never mounted" (real HTML shell, no rendered content) from
      // "navigated to the wrong thing entirely".
      const url = await browser.getUrl().catch((e) => `<getUrl failed: ${e}>`);
      const source = await browser
        .getPageSource()
        .then((s) => s.slice(0, 2000))
        .catch((e) => `<getPageSource failed: ${e}>`);
      console.log(`[diagnostic] current URL: ${url}`);
      console.log(`[diagnostic] page source (first 2000 chars): ${source}`);
      throw err;
    }
    await expect(heading).toHaveText("Doce");

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
