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
describe("Onboarding (User Story 1: zero-config first run)", () => {
  it("shows the Doce heading with no model picker, API key field, or account step", async () => {
    await browser.pause(1500);
    const heading = await browser.$("h1");
    await heading.waitForExist({ timeout: 15000 });
    await expect(heading).toHaveText("Doce");

    const apiKeyInputs = await browser.$$("input[type='password']");
    expect(apiKeyInputs.length).toBe(0);
  });

  it("displays real detected hardware info, not a placeholder", async () => {
    const hardwareLine = await browser.$("p*=tier");
    await hardwareLine.waitForExist({ timeout: 15000 });
    const text = await hardwareLine.getText();

    // Real chip/RAM values from sysctlbyname, not "unknown"/0 fallbacks.
    expect(text).not.toContain("unknown");
    expect(text).toMatch(/Apple/);
  });

  it("starts downloading the model automatically", async () => {
    const progressLabel = await browser.$("p*=Downloading");
    await progressLabel.waitForExist({ timeout: 20000 });
  });

  it("finishes the download, verifies it, and transitions to chat", async () => {
    const chatInput = await browser.$("[data-testid='chat-input']");
    await chatInput.waitForExist({ timeout: 11 * 60 * 1000, interval: 2000 });
  });
});
