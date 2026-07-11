import { expect } from "@wdio/globals";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { startWorkspaceConversationViaComposer } from "./helpers";

// Covers specs/007-workspace-cwd-resolution: proves the real, non-mocked
// path end to end — send_agent_message resolves the conversation's
// workspace path, threads it through AgentContext into dispatch::execute,
// and bash::run spawns with it as the working directory. Distinct from
// open-folder.spec.ts (which reads a file via its *absolute* path — that
// exercises FR-004's "absolute paths are unaffected," not this feature's
// actual relative/default-path resolution). Entry point updated for
// 006-chat-empty-state: every workspace-scoped conversation now starts via
// the composer, folder and first message together.
describe("Workspace cwd resolution (007): Bash reflects the chosen folder", () => {
  it("running `ls .` via the Bash tool lists the chosen folder's contents, not the app's own", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-cwd-e2e-"));
    const markerName = "DOCE_E2E_CWD_MARKER.txt";
    writeFileSync(path.join(dir, markerName), "marker file for 007-workspace-cwd-resolution");

    await startWorkspaceConversationViaComposer(
      dir,
      "Run the command `ls .` using the Bash tool (a relative reference, not an absolute path) and tell me exactly what it printed, verbatim.",
    );

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
    // If Bash had spawned with the app's own ambient cwd instead of the
    // chosen folder (the bug this feature fixes), this marker would never
    // appear — `ls .` would have listed something else entirely.
    expect(combined).toContain(markerName);
  });
});
