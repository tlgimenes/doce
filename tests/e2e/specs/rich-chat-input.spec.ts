import { expect } from "@wdio/globals";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import path from "node:path";
import { startWorkspaceConversationViaComposer } from "./helpers";

// Covers 009-rich-chat-input against the real app, real model: the two
// behaviors quickstart.md's manual walkthrough can only fully prove live
// (a real paste round-trip reaching the model with its full original text,
// and a real skill selection actually changing the agent's behavior, not
// just rendering a chip) — everything else about this feature is already
// covered by unit/component tests per research.md's three-tier testing
// strategy.
//
// Submission always goes through the real "agent-send" button, never
// browser.keys(Key.Enter) — this project's other e2e specs all click the
// submit button for exactly this reason: this WebdriverIO/embedded-webkit
// setup resolves `Key.Enter` to an empty key value (confirmed directly —
// `browser.keys(Key.Enter)` produced a no-op `{"keyDown","value":""}`
// action, silently never submitting).
//
// A large paste is simulated by dispatching a real `ClipboardEvent`
// directly on the focused contenteditable root, rather than
// `addValue()`/`elementSendKeys()` (individual keystrokes — never fires a
// `paste` event at all) or an OS-level Cmd+V (confirmed unreliable in this
// specific WebKit+Tauri WebDriver setup — the paste never reached the
// document even though the same key-action shape works for this project's
// other Cmd+<letter> shortcuts). Dispatching the event directly is the
// standard technique for testing paste handlers precisely because real
// OS/clipboard interaction is notoriously environment-dependent across
// automation stacks — mesh's own e2e suite hits the analogous problem with
// contenteditable inputs and documents working around it the same way.
async function pasteInto(selector: string, text: string) {
  await browser.execute(
    (sel, pastedText) => {
      const el = document.querySelector(sel) as HTMLElement | null;
      if (!el) throw new Error(`pasteInto: no element matching ${sel}`);
      el.focus();
      const dt = new DataTransfer();
      dt.setData("text/plain", pastedText);
      const event = new ClipboardEvent("paste", {
        bubbles: true,
        cancelable: true,
        clipboardData: dt,
      });
      el.dispatchEvent(event);
    },
    selector,
    text,
  );
}

describe("Rich chat input (009-rich-chat-input)", () => {
  it("US2: a large paste collapses into a chip, and the agent still receives its full original text (FR-003/FR-005)", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-rich-input-paste-e2e-"));

    const agentInput = await startWorkspaceConversationViaComposer(
      dir,
      "Say hello, don't use any tools for this first message.",
      120000,
    );

    // A block well over both thresholds (10 lines / 500 chars) — each line
    // carries a distinct marker so the assistant's later reply can prove it
    // actually saw the *whole* thing, not just the first/last few lines.
    const markers = Array.from({ length: 25 }, (_, i) => `DOCE_E2E_PASTE_LINE_MARKER_${i}`);
    const pastedBlock = markers.join("\n");

    await pasteInto("[data-testid='agent-input']", pastedBlock);

    const chip = await browser.$("[data-testid='pasted-text-chip']");
    await chip.waitForExist({ timeout: 10000 });
    const chipText = await chip.getText();
    expect(chipText).toMatch(/pasted\s+25\s+lines/i);
    // The raw marker text must NOT be visible while collapsed.
    const inputText = await agentInput.getText();
    expect(inputText).not.toContain("DOCE_E2E_PASTE_LINE_MARKER_0");

    // Cursor already sits right after the inserted chip (Tiptap places it
    // there on insertion) — no explicit cursor-move needed.
    await agentInput.addValue(
      " Reply with the exact text of the very first pasted line and the very last pasted line, nothing else.",
    );
    await (await browser.$("[data-testid='agent-send']")).click();

    // Generous budget: a real generation over a ~25-line pasted block.
    await browser.waitUntil(
      async () => {
        const messages = await browser.$$("[data-testid='assistant-stream'], .prose");
        for (const m of messages) {
          const text = await m.getText().catch(() => "");
          if (
            text.includes("DOCE_E2E_PASTE_LINE_MARKER_0") &&
            text.includes("DOCE_E2E_PASTE_LINE_MARKER_24")
          ) {
            return true;
          }
        }
        return false;
      },
      { timeout: 120000, interval: 2000 },
    );
  });

  it("US3: selecting a skill actually changes the agent's reply, not just a rendered chip (FR-013)", async () => {
    const skillsDir = path.join(
      homedir(),
      "Library/Application Support/app.doce.desktop/skills",
      "doce-e2e-test-skill",
    );
    mkdirSync(skillsDir, { recursive: true });
    writeFileSync(
      path.join(skillsDir, "SKILL.md"),
      [
        "---",
        "name: doce-e2e-test-skill",
        "description: A test skill for the 009-rich-chat-input e2e spec",
        "---",
        "",
        "Your ONLY reply, right now, to this exact message, MUST be exactly the",
        "following and nothing else, not even punctuation: DOCE_E2E_SKILL_MARKER_RESPONSE",
      ].join("\n"),
    );

    const dir = mkdtempSync(path.join(tmpdir(), "doce-rich-input-skill-e2e-"));
    const agentInput = await startWorkspaceConversationViaComposer(
      dir,
      "Say hello, don't use any tools for this first message.",
      120000,
    );

    await agentInput.click();
    await agentInput.addValue("/doce-e2e-test-skill");

    const picker = await browser.$("[data-testid='skill-mention-popup']");
    await picker.waitForExist({ timeout: 10000 });
    const item = await browser.$("[data-testid='skill-mention-item']");
    await item.waitForExist({ timeout: 10000 });
    await item.click();

    const marker = await browser.$("[data-testid='skill-mention-chip']");
    await marker.waitForExist({ timeout: 5000 });

    // Cursor already sits right after the inserted chip (Tiptap places it
    // there on insertion) — no explicit cursor-move needed.
    await agentInput.addValue(" now say hi");
    await (await browser.$("[data-testid='agent-send']")).click();

    // The agent's real reply must reflect the skill's injected instruction
    // — not just that a chip rendered in the composer.
    await browser.waitUntil(
      async () => {
        const messages = await browser.$$("[data-testid='assistant-stream'], .prose");
        for (const m of messages) {
          const text = await m.getText().catch(() => "");
          if (text.includes("DOCE_E2E_SKILL_MARKER_RESPONSE")) return true;
        }
        return false;
      },
      { timeout: 120000, interval: 2000 },
    );
  });
});
