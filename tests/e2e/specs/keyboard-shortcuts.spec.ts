import { expect } from "@wdio/globals";
import { Key } from "webdriverio";

// Covers specs/005-keyboard-shortcuts: proves the global shortcuts
// against the real, running app in real WebKit — not jsdom, which has no
// implementation of <dialog>'s interactive behavior at all (App.test.tsx's
// unit coverage relies on a polyfill for that reason; this spec is the
// live check that the real thing actually works).
async function waitForEditableWorkspaceInput() {
  const input = await browser.$("[data-testid='agent-input']");
  await input.waitForExist({ timeout: 60000 });
  await browser.waitUntil(async () => (await input.getAttribute("contenteditable")) === "true", {
    timeout: 60000,
    timeoutMsg: "workspace input never became editable",
  });
  return input;
}

describe("Keyboard shortcuts (005)", () => {
  it("Cmd+N opens the empty-state composer", async () => {
    await browser.keys([Key.Command, "n"]);

    const input = await browser.$("[data-testid='empty-state-input']");
    await input.waitForExist({ timeout: 10000 });
    expect(await input.isExisting()).toBe(true);
  });

  it("Cmd+L focuses the workspace input from elsewhere on the page (US1)", async () => {
    await browser.keys([Key.Command, "n"]);
    const emptyInput = await browser.$("[data-testid='empty-state-input']");
    await emptyInput.waitForExist({ timeout: 10000 });
    await emptyInput.setValue("DOCE_E2E_SHORTCUT_FOCUS create a workspace conversation");
    await (await browser.$("[data-testid='empty-state-submit']")).click();

    const input = await waitForEditableWorkspaceInput();

    const sidebar = await browser.$("[data-testid='conversation-list']");
    await sidebar.click();
    expect(await input.isFocused()).toBe(false);

    await browser.keys([Key.Command, "l"]);

    await browser.waitUntil(async () => input.isFocused(), {
      timeout: 5000,
      timeoutMsg: "Cmd+L never focused the workspace input",
    });
  });

  it("Cmd+K opens the shortcuts dialog listing all four shortcuts; Cmd+K again closes it (US3, FR-006)", async () => {
    await browser.keys([Key.Command, "k"]);

    const dialog = await browser.$("[data-testid='shortcuts-dialog']");
    await dialog.waitForExist({ timeout: 10000 });
    const rows = await browser.$$("[data-testid='shortcut-item']");
    expect(rows.length).toBe(4);

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
