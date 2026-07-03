import { expect } from "@wdio/globals";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

// Covers specs/007-workspace-cwd-resolution: proves the real, non-mocked
// path end to end — send_agent_message resolves the conversation's
// workspace path, threads it through AgentContext into dispatch::execute,
// and bash::run spawns with it as the working directory. Distinct from
// agent-mode.spec.ts (which reads a file via its *absolute* path — that
// exercises FR-004's "absolute paths are unaffected," not this feature's
// actual relative/default-path resolution).
describe("Workspace cwd resolution (007): Bash reflects the chosen folder", () => {
  it("running `ls .` via the Bash tool lists the chosen folder's contents, not the app's own", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-cwd-e2e-"));
    const markerName = "DOCE_E2E_CWD_MARKER.txt";
    writeFileSync(path.join(dir, markerName), "marker file for 007-workspace-cwd-resolution");

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
      "Run the command `ls .` using the Bash tool (a relative reference, not an absolute path) and tell me exactly what it printed, verbatim.",
    );
    await (await browser.$("[data-testid='agent-send']")).click();

    await browser.waitUntil(
      async () => !(await browser.$("[data-testid='agent-thinking']").isExisting()),
      { timeout: 90000, timeoutMsg: "agent never finished responding" },
    );

    const bubbles = await browser.$$("[data-testid='chat-message']");
    const texts: string[] = [];
    for (let i = 0; i < bubbles.length; i++) {
      texts.push(await bubbles[i].getText());
    }
    const combined = texts.join("\n");
    // If Bash had spawned with the app's own ambient cwd instead of the
    // chosen folder (the bug this feature fixes), this marker would never
    // appear — `ls .` would have listed something else entirely.
    expect(combined).toContain(markerName);
  });
});
