import { expect } from "@wdio/globals";

// Covers quickstart.md §5 (User Story 7): the conversation-list sidebar,
// creating new threads, and status dots. Also the direct regression check
// for the reported bug — "unable to create new threads on the ui" — which
// was real: there was no sidebar or new-thread control at all before this.
describe("Conversation list (User Story 7)", () => {
  it("shows the sidebar with a working New conversation action", async () => {
    const sidebar = await browser.$("[data-testid='conversation-list']");
    await sidebar.waitForExist({ timeout: 15000 });

    const newButton = await browser.$("[data-testid='new-conversation']");
    await newButton.waitForExist({ timeout: 5000 });
  });

  it("creates a new, distinct thread each time it's clicked", async () => {
    const newButton = await browser.$("[data-testid='new-conversation']");

    await newButton.click();
    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='conversation-item']")).length >= 1,
      { timeout: 15000, timeoutMsg: "first thread never appeared in the sidebar" },
    );
    const afterFirst = (await browser.$$("[data-testid='conversation-item']")).length;

    await newButton.click();
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

  it("selecting a thread shows its own chat input", async () => {
    const items = await browser.$$("[data-testid='conversation-item']");
    await items[0].click();

    const input = await browser.$("[data-testid='chat-input']");
    await input.waitForExist({ timeout: 15000 });
  });
});
