/**
 * 006-chat-empty-state removed the free-text workspace-path entry
 * (`workspace-path-input`/`open-workspace`) these specs used to drive
 * directly — folders are now chosen via the composer's recents picker or
 * the native OS browse dialog, and the latter is outside the webview so
 * WebDriver can't automate it. This seeds the temp dir as a "recently
 * used" workspace through the app's own real `open_workspace` command
 * (the exact command a human opening it once before would have gone
 * through), then drives the rest — picking it from the picker, typing the
 * task, submitting — through real UI like a user would.
 */
export async function startWorkspaceConversationViaComposer(
  dir: string,
  taskText: string,
  firstTurnTimeoutMs = 90000,
) {
  await browser.execute((path) => {
    // window.__TAURI_INTERNALS__ is always injected into a Tauri webview
    // regardless of the `withGlobalTauri` config — it's what
    // @tauri-apps/api/core's own `invoke()` calls internally.
    return (window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args: unknown) => Promise<unknown> } })
      .__TAURI_INTERNALS__.invoke("open_workspace", { path });
  }, dir);

  const selector = await browser.$("[data-testid='folder-target-selector']");
  await selector.waitForExist({ timeout: 15000 });
  await selector.click();

  const picker = await browser.$("[data-testid='folder-picker']");
  await picker.waitForExist({ timeout: 10000 });
  await (await browser.$("[data-testid='folder-picker-filter']")).setValue(dir);

  const item = await browser.$(`[data-testid='folder-picker-item'][title='${dir}']`);
  await item.waitForExist({ timeout: 10000 });
  await item.click();

  const input = await browser.$("[data-testid='empty-state-input']");
  await input.waitForExist({ timeout: 10000 });
  await input.setValue(taskText);
  await (await browser.$("[data-testid='empty-state-submit']")).click();

  // The composer only switches to the workspace view once the full
  // open_workspace -> create_conversation -> send_agent_message sequence
  // completes (FR-003: one atomic action) — including the real first turn,
  // not just opening the folder, so this needs the same generous budget as
  // waiting out "agent-thinking" elsewhere in these specs.
  const agentInput = await browser.$("[data-testid='agent-input']");
  await agentInput.waitForExist({ timeout: firstTurnTimeoutMs });
  return agentInput;
}
