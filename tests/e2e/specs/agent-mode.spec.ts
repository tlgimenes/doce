import { expect } from "@wdio/globals";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

// Covers quickstart.md §3 (User Story 3: agent mode): opening a real folder
// and giving the agent a task that requires it to actually use a real
// built-in tool (Read) against a real file — not a mocked backend, the
// real tool-use loop in src-tauri/src/agent/mod.rs against the real
// installed model.
describe("Agent mode (User Story 3: open a folder to enter agent mode)", () => {
  it("reads a real file via the Read tool to answer a question about its contents", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-agent-e2e-"));
    const markerContent = "The secret ingredient is DOCE_E2E_MARKER_PANCAKES.";
    writeFileSync(path.join(dir, "notes.txt"), markerContent);

    const enterAgentMode = await browser.$("[data-testid='enter-agent-mode']");
    await enterAgentMode.waitForExist({ timeout: 15000 });
    await enterAgentMode.click();

    const pathInput = await browser.$("[data-testid='workspace-path-input']");
    await pathInput.waitForExist({ timeout: 10000 });
    await pathInput.setValue(dir);
    await (await browser.$("[data-testid='open-workspace']")).click();

    const agentInput = await browser.$("[data-testid='agent-input']");
    await agentInput.waitForExist({ timeout: 15000 });
    await agentInput.setValue(`Read the file ${path.join(dir, "notes.txt")} and tell me what the secret ingredient is.`);
    await (await browser.$("[data-testid='agent-send']")).click();

    // Generous timeout: this may involve multiple tool-call turns, each
    // paying for a real (small but real) model generation.
    await browser.waitUntil(
      async () => !(await browser.$("[data-testid='agent-thinking']").isExisting()),
      { timeout: 90000, timeoutMsg: "agent never finished responding" },
    );

    const bubbles = await browser.$$("div.mb-3");
    const texts: string[] = [];
    for (let i = 0; i < bubbles.length; i++) {
      texts.push(await bubbles[i].getText());
    }
    const combined = texts.join("\n");
    expect(combined).toContain("DOCE_E2E_MARKER_PANCAKES");
  });
});
