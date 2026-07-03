import { expect } from "@wdio/globals";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { startWorkspaceConversationViaComposer } from "./helpers";

// Covers 004-tool-call-widgets' core ask ("tool calls aren't rendering on
// the UI at all") against the real app, real model, real tools — not just
// the unit-level detail-shape/widget-rendering coverage. US1 (Edit -> real
// diff) and US2 (Bash -> real terminal widget) are the two highest-value
// widgets per spec.md's own prioritization.
describe("Tool call widgets (004-tool-call-widgets)", () => {
  it("US1: a real file edit renders as a labeled diff, not plain text (FR-002)", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-widgets-edit-e2e-"));
    const file = path.join(dir, "greeting.txt");
    writeFileSync(file, "Hello DOCE_E2E_WIDGET_MARKER_OLD\n");

    await startWorkspaceConversationViaComposer(
      dir,
      `Edit the file ${file} using the Edit tool: replace the exact text "DOCE_E2E_WIDGET_MARKER_OLD" with "DOCE_E2E_WIDGET_MARKER_NEW".`,
      120000,
    );

    const diff = await browser.$("[data-testid='edit-diff']");
    await diff.waitForExist({ timeout: 15000 });
    const removedText = await (await browser.$("[data-testid='diff-removed']")).getText();
    const addedText = await (await browser.$("[data-testid='diff-added']")).getText();
    expect(removedText).toContain("DOCE_E2E_WIDGET_MARKER_OLD");
    expect(addedText).toContain("DOCE_E2E_WIDGET_MARKER_NEW");
  });

  it("US2: a real shell command renders as a terminal-style widget with visible exit status (FR-003)", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-widgets-bash-e2e-"));
    writeFileSync(path.join(dir, "DOCE_E2E_WIDGET_BASH_MARKER.txt"), "marker");

    await startWorkspaceConversationViaComposer(
      dir,
      "Run the command `ls .` using the Bash tool and tell me what it printed.",
      90000,
    );

    const bash = await browser.$("[data-testid='bash-widget']");
    await bash.waitForExist({ timeout: 15000 });
    const statusText = await (await browser.$("[data-testid='bash-status']")).getText();
    expect(statusText.toLowerCase()).toContain("success");
    const stdoutText = await (await browser.$("[data-testid='bash-stdout']")).getText();
    expect(stdoutText).toContain("DOCE_E2E_WIDGET_BASH_MARKER.txt");
  });
});
