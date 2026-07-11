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

    // Completed Bash widgets are collapsed by default with the body
    // unmounted (Base UI) -- click the header trigger to expand before
    // reading stdout.
    const bashHeader = await browser.$("[data-testid='bash-widget'] [role='button']");
    await bashHeader.click();
    const stdout = await browser.$("[data-testid='bash-stdout']");
    await stdout.waitForExist({ timeout: 10000 });
    const stdoutText = await stdout.getText();
    expect(stdoutText).toContain("DOCE_E2E_WIDGET_BASH_MARKER.txt");
  });

  // Regression: found live, not speculatively. `send_agent_message` blocks
  // on `rx.await` the moment the model calls `AskUserQuestion` (001's
  // FR-010 pause/resume mechanic) -- and until this fix, nothing in the UI
  // ever showed that pending question or let a user answer it, so the
  // whole turn (and the one global inference-engine lock it holds for as
  // long as it's blocked) hung forever with no error and no way out.
  it("a real AskUserQuestion pauses the loop with a visible, answerable prompt, and answering it resumes and completes the turn", async () => {
    const dir = mkdtempSync(path.join(tmpdir(), "doce-widgets-question-e2e-"));

    const agentInput = await startWorkspaceConversationViaComposer(dir, "Say hi in one word.");
    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='chat-message']")).length >= 2,
      { timeout: 30000, timeoutMsg: "first turn never completed" },
    );

    await agentInput.setValue(
      "Use the AskUserQuestion tool right now: header 'Quick check', question " +
        "'Which color do you prefer?', options 'Red' and 'Blue', not multi-select. " +
        "Do not answer it yourself -- actually call the tool and wait.",
    );
    await (await browser.$("[data-testid='agent-send']")).click();

    const widget = await browser.$("[data-testid='user-ask-widget']");
    await widget.waitForExist({ timeout: 60000 });
    expect(await widget.getText()).toContain("Which color do you prefer?");

    // The regression itself: the composer must be fully replaced by the
    // question widget while genuinely paused here, not merely disabled --
    // typing into a still-present-but-disabled input would queue a message
    // up stuck behind the same lock rather than doing anything.
    const composerInputWhilePending = await browser.$("[data-testid='agent-input']");
    expect(await composerInputWhilePending.isExisting()).toBe(false);

    // The 2026-07-08 redesign made picking an option select-only (never
    // auto-submit) for both single- and multi-select -- "Red" is now a
    // Field/FieldLabel row's text, not a <button>, and text lives two
    // levels deep (FieldContent > FieldTitle), so it isn't a direct text
    // node of the `question-option` element: a strict `=Red` match (xpath
    // `text()`) finds nothing, only the partial `*=Red` match (xpath
    // `contains(.)`) does. Answering also now requires an explicit,
    // separate submit click.
    await (await widget.$("[data-testid='question-option']*=Red")).click();
    await (await widget.$("[data-testid='question-submit']")).click();

    // Answering must actually resume the loop and let the turn finish --
    // not just flip the widget to "answered" while the backend stays
    // blocked underneath it.
    const answered = await browser.$("[data-testid='question-answered']");
    await answered.waitForExist({ timeout: 60000 });
    expect(await answered.getText()).toContain("Red");
  });
});
