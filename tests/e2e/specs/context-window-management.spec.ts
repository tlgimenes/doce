import { expect } from "@wdio/globals";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { startWorkspaceConversationViaComposer } from "./helpers";

// Covers specs/010-context-window-management/quickstart.md's manual
// validation steps for US1 (live visibility) and US3 (tool-output
// offloading) against the real app: a real installed model, the real
// agent tool-use loop, and the real context-usage/offload plumbing — no
// mocked backend, no stubbed IPC.
//
// Submission goes through the real "agent-send" button, never
// browser.keys(Key.Enter) — see rich-chat-input.spec.ts's comment on why
// (Key.Enter is a confirmed no-op in this WebDriver+WebKit+Tauri setup).
// This also means these specs can't exercise the /compact-vs-skill-picker
// Enter-key interaction live; that's covered instead by
// skill-mention.test.tsx's jsdom-level tests, which do dispatch real
// keydown events.
describe("Context window management (010-context-window-management)", () => {
  it("shows a live context-usage gauge with a nonzero percentage after a real turn", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-context-e2e-"));

    await startWorkspaceConversationViaComposer(dir, "Say hello in exactly three words.");

    const gauge = await browser.$("[data-testid='context-usage-gauge']");
    await gauge.waitForExist({ timeout: 30000 });

    const ariaLabel = await gauge.getAttribute("aria-label");
    const percentMatch = ariaLabel?.match(/(\d+)%/);
    expect(percentMatch).not.toBeNull();
    expect(parseInt(percentMatch![1], 10)).toBeGreaterThan(0);
  });

  it("typing /compact and clicking send triggers compaction directly rather than sending it as a normal message", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-context-e2e-"));

    const agentInput = await startWorkspaceConversationViaComposer(dir, "Say hi in one word.");

    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='chat-message']")).length >= 2,
      {
        timeout: 30000,
        timeoutMsg: "first turn never completed",
      },
    );
    const messageCountBefore = (await browser.$$("[data-testid='chat-message']")).length;

    await agentInput.setValue("/compact");
    await (await browser.$("[data-testid='agent-send']")).click();

    // A no-op compaction (nothing eligible to clear/summarize yet in such a
    // short conversation) persists nothing new — the message count must
    // stay exactly where it was, and specifically never grow by a literal
    // "/compact" user bubble.
    await browser.pause(3000);
    const bubbles = await browser.$$("[data-testid='chat-message']");
    // A manual for-loop, not `.map()` — webdriverio's `ElementArray` isn't a
    // plain iterable Array (workspace-chat.spec.ts's own `bubbleTexts()`
    // helper uses the same pattern for exactly this reason).
    const texts: string[] = [];
    for (let i = 0; i < bubbles.length; i++) {
      texts.push(await bubbles[i].getText());
    }
    expect(texts.some((t) => t.includes("/compact"))).toBe(false);
    expect(bubbles.length).toBe(messageCountBefore);
  });

  it("offloads a large Bash tool result and shows a working 'View full output' affordance", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-context-e2e-"));

    await startWorkspaceConversationViaComposer(
      dir,
      "Run the shell command: yes x | head -c 3000",
      120000,
    );

    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='chat-message']")).length >= 2,
      {
        timeout: 30000,
        timeoutMsg: "messages never loaded after the composer created the conversation",
      },
    );

    const viewFullOutputButton = await browser.$("[data-testid='view-full-output-button']");
    await viewFullOutputButton.waitForExist({ timeout: 15000 });
    await viewFullOutputButton.click();

    const content = await browser.$("[data-testid='view-full-output-content']");
    await content.waitForExist({ timeout: 15000 });
    const fullText = await content.getText();
    // The full offloaded output must contain far more of the repeated
    // marker character than any inline preview would — proving this is the
    // real full file content, not just the same short preview re-rendered.
    const xCount = (fullText.match(/x/g) ?? []).length;
    expect(xCount).toBeGreaterThan(600);
  });
});
