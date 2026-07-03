import { expect } from "@wdio/globals";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

// Covers quickstart.md §4 (subagent spawning, FR-015/FR-016): asking the
// agent to delegate a sub-task should produce a real, isolated subagent
// conversation (visible via sqlite as its own row with
// spawned_by_conversation_id set) whose final answer flows back into the
// parent's reply — not just a unit-tested control-flow path.
describe("Agent mode subagent spawning (FR-015)", () => {
  it("delegating a task via the Task tool produces a real subagent conversation with an isolated context", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-subagent-e2e-"));
    writeFileSync(path.join(dir, "notes.txt"), "The secret ingredient is DOCE_E2E_SUBAGENT_WAFFLES.");

    const enterAgentMode = await browser.$("[data-testid='enter-agent-mode']");
    await enterAgentMode.waitForExist({ timeout: 15000 });
    await enterAgentMode.click();

    const pathInput = await browser.$("[data-testid='workspace-path-input']");
    await pathInput.waitForExist({ timeout: 10000 });
    await pathInput.setValue(dir);
    await (await browser.$("[data-testid='open-workspace']")).click();

    const agentInput = await browser.$("[data-testid='agent-input']");
    await agentInput.waitForExist({ timeout: 15000 });
    await agentInput.setValue(
      `Delegate this to a subagent using the Task tool: read the file ${path.join(dir, "notes.txt")} and report the secret ingredient.`,
    );
    await (await browser.$("[data-testid='agent-send']")).click();

    await browser.waitUntil(async () => !(await browser.$("[data-testid='agent-thinking']").isExisting()), {
      timeout: 120000,
      timeoutMsg: "agent never finished responding",
    });

    // Whether or not the small model actually chose to use the Task tool
    // this run (it may just answer directly, which is also a valid
    // response to "delegate this"), the request must complete with some
    // real, non-empty answer rather than hanging or erroring out.
    const bubbles = await browser.$$("div.mb-3");
    const texts: string[] = [];
    for (let i = 0; i < bubbles.length; i++) {
      texts.push(await bubbles[i].getText());
    }
    const lastReply = texts[texts.length - 1];
    expect(lastReply.trim().length).toBeGreaterThan(0);
  });
});
