import { expect } from "@wdio/globals";

// Covers quickstart.md §5 (User Story 7): the conversation-list sidebar,
// creating new threads, and status dots. Also the direct regression check
// for the reported bug — "unable to create new threads on the ui" — which
// was real: there was no sidebar or new-thread control at all before this.
//
// 006-chat-empty-state changed what "+ New conversation" does: it shows
// the composer instead of instantly creating a row (FR-002), and actually
// creating a thread now requires submitting a real first message — so this
// spec drives that through, real model turns included, rather than the old
// single instant click.
async function submitComposer(text: string) {
  const newButton = await browser.$("[data-testid='new-conversation']");
  await newButton.click();

  const input = await browser.$("[data-testid='empty-state-input']");
  await input.waitForExist({ timeout: 10000 });
  await input.setValue(text);
  await (await browser.$("[data-testid='empty-state-submit']")).click();

  // Home as the target (untouched) — resolves to a real, always-existing
  // directory, so no picker interaction is needed for this spec's purpose.
  const agentInput = await browser.$("[data-testid='agent-input']");
  await agentInput.waitForExist({ timeout: 60000 });
}

describe("Conversation list (User Story 7)", () => {
  it("shows the sidebar, and '+ New conversation' shows the composer rather than instantly creating a thread (006 FR-002)", async () => {
    const sidebar = await browser.$("[data-testid='conversation-list']");
    await sidebar.waitForExist({ timeout: 15000 });

    const before = (await browser.$$("[data-testid='conversation-item']")).length;
    await (await browser.$("[data-testid='new-conversation']")).click();

    await browser.$("[data-testid='empty-state-input']").then((el) => el.waitForExist({ timeout: 10000 }));
    expect((await browser.$$("[data-testid='conversation-item']")).length).toBe(before);
  });

  it("creates a new, distinct thread each time the composer is submitted", async () => {
    await submitComposer("DOCE_E2E_THREAD_ONE say hello in exactly three words");
    const afterFirst = (await browser.$$("[data-testid='conversation-item']")).length;
    expect(afterFirst).toBeGreaterThanOrEqual(1);

    await submitComposer("DOCE_E2E_THREAD_TWO say hello in exactly three words");
    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='conversation-item']")).length > afterFirst,
      { timeout: 15000, timeoutMsg: "second thread never appeared — this is the exact reported bug" },
    );

    const items = await browser.$$("[data-testid='conversation-item']");
    const ids = new Set<string>();
    for (let i = 0; i < items.length; i++) {
      ids.add(await items[i].getAttribute("data-conversation-id"));
    }
    expect(ids.size).toBe(items.length);
  });

  it("selecting a thread shows its own agent input (every new thread is workspace-scoped, 006 FR-004)", async () => {
    const items = await browser.$$("[data-testid='conversation-item']");
    await items[0].click();

    const input = await browser.$("[data-testid='agent-input']");
    await input.waitForExist({ timeout: 15000 });
  });
});
