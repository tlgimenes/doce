import { expect } from "@wdio/globals";
import { Key } from "webdriverio";

// Covers specs/005-keyboard-shortcuts: proves the three global shortcuts
// against the real, running app in real WebKit — not jsdom, which has no
// implementation of <dialog>'s interactive behavior at all (App.test.tsx's
// unit coverage relies on a polyfill for that reason; this spec is the
// live check that the real thing actually works).
describe("Keyboard shortcuts (005)", () => {
  it("Cmd+N creates a new conversation and switches to it (US2)", async () => {
    const before = (await browser.$$("[data-testid='conversation-item']")).length;

    await browser.keys([Key.Command, "n"]);

    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='conversation-item']")).length > before,
      { timeout: 15000, timeoutMsg: "Cmd+N never created a new conversation" },
    );
    const input = await browser.$("[data-testid='chat-input']");
    await input.waitForExist({ timeout: 10000 });
  });

  it("Cmd+L focuses the chat input from elsewhere on the page (US1)", async () => {
    const sidebar = await browser.$("[data-testid='conversation-list']");
    await sidebar.click();

    const input = await browser.$("[data-testid='chat-input']");
    await input.waitForExist({ timeout: 10000 });
    expect(await input.isFocused()).toBe(false);

    await browser.keys([Key.Command, "l"]);

    await browser.waitUntil(async () => input.isFocused(), {
      timeout: 5000,
      timeoutMsg: "Cmd+L never focused the chat input",
    });
  });

  it("Cmd+K opens the shortcuts dialog listing all three shortcuts; Cmd+K again closes it (US3, FR-006)", async () => {
    await browser.keys([Key.Command, "k"]);

    const dialog = await browser.$("[data-testid='shortcuts-dialog']");
    await dialog.waitForExist({ timeout: 10000 });
    const rows = await browser.$$("[data-testid='shortcut-item']");
    expect(rows.length).toBe(3);

    await browser.keys([Key.Command, "k"]);
    await browser.waitUntil(async () => !(await dialog.isExisting()), {
      timeout: 5000,
      timeoutMsg: "shortcuts dialog never closed on a second Cmd+K",
    });
  });

  it("Escape and the close button both dismiss the shortcuts dialog (FR-005)", async () => {
    await browser.keys([Key.Command, "k"]);
    let dialog = await browser.$("[data-testid='shortcuts-dialog']");
    await dialog.waitForExist({ timeout: 10000 });

    await browser.keys([Key.Escape]);
    await browser.waitUntil(async () => !(await dialog.isExisting()), {
      timeout: 5000,
      timeoutMsg: "Escape never closed the shortcuts dialog",
    });

    await browser.keys([Key.Command, "k"]);
    dialog = await browser.$("[data-testid='shortcuts-dialog']");
    await dialog.waitForExist({ timeout: 10000 });
    await (await browser.$("[data-testid='close-shortcuts-dialog']")).click();
    await browser.waitUntil(async () => !(await dialog.isExisting()), {
      timeout: 5000,
      timeoutMsg: "the close button never closed the shortcuts dialog",
    });
  });
});
