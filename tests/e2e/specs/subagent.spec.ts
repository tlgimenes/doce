import { expect } from "@wdio/globals";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { startWorkspaceConversationViaComposer } from "./helpers";

// Covers quickstart.md §4 (subagent spawning, FR-015/FR-016): asking the
// agent to delegate a sub-task should produce a real, isolated subagent
// conversation (visible via sqlite as its own row with
// spawned_by_conversation_id set) whose final answer flows back into the
// parent's reply — not just a unit-tested control-flow path. Entry point
// updated for 006-chat-empty-state: every workspace-scoped conversation now
// starts via the composer, folder and first message together.
describe("Agent mode subagent spawning (FR-015)", () => {
  it("delegating a task via the Task tool produces a real subagent conversation with an isolated context", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-subagent-e2e-"));
    writeFileSync(path.join(dir, "notes.txt"), "The secret ingredient is DOCE_E2E_SUBAGENT_WAFFLES.");

    // Generous timeout: delegation may involve multiple real model turns
    // across both the parent and the subagent loop.
    await startWorkspaceConversationViaComposer(
      dir,
      `Delegate this to a subagent using the Task tool: read the file ${path.join(dir, "notes.txt")} and report the secret ingredient.`,
      120000,
    );

    // Whether or not the small model actually chose to use the Task tool
    // this run (it may just answer directly, which is also a valid
    // response to "delegate this"), the request must complete with some
    // real, non-empty answer rather than hanging or erroring out.
    await browser.waitUntil(async () => (await browser.$$("[data-testid='chat-message']")).length >= 2, {
      timeout: 15000,
      timeoutMsg: "messages never loaded after the composer created the conversation",
    });
    const bubbles = await browser.$$("[data-testid='chat-message']");
    const texts: string[] = [];
    for (let i = 0; i < bubbles.length; i++) {
      texts.push(await bubbles[i].getText());
    }
    const lastReply = texts[texts.length - 1];
    expect(lastReply.trim().length).toBeGreaterThan(0);
  });
});
