import { expect } from "@wdio/globals";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { startWorkspaceConversationViaComposer } from "./helpers";

// Covers quickstart.md §3 (User Story 3): opening a real folder and giving
// the agent a task that requires it to actually use a real built-in tool
// (Read) against a real file — not a mocked backend, the real tool-use
// loop in src-tauri/src/agent/mod.rs against the real installed model.
// Entry point updated for 006-chat-empty-state: every workspace-scoped
// conversation now starts via the composer, folder and first message
// together, not a separate "open a folder" step.
describe("Open a folder (User Story 3: workspace conversation uses real tools)", () => {
  it("reads a real file via the Read tool to answer a question about its contents", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-agent-e2e-"));
    const markerContent = "The secret ingredient is DOCE_E2E_MARKER_PANCAKES.";
    writeFileSync(path.join(dir, "notes.txt"), markerContent);

    await startWorkspaceConversationViaComposer(
      dir,
      `Read the file ${path.join(dir, "notes.txt")} and tell me what the secret ingredient is.`,
    );

    // The composer only switches to this view once the full turn (which may
    // involve multiple real tool-call round-trips) has already completed —
    // just wait for the resulting bubbles to render.
    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='chat-message']")).length >= 2,
      {
        timeout: 15000,
        timeoutMsg: "messages never loaded after the composer created the conversation",
      },
    );
    const bubbles = await browser.$$("[data-testid='chat-message']");
    const texts: string[] = [];
    for (let i = 0; i < bubbles.length; i++) {
      texts.push(await bubbles[i].getText());
    }
    const combined = texts.join("\n");
    expect(combined).toContain("DOCE_E2E_MARKER_PANCAKES");
  });
});
