import { expect } from "@wdio/globals";
import { Key } from "webdriverio";

const MARKER_ONE = "DOCE_E2E_WORKSPACE_MARKER_ONE say hello in exactly three words";
const MARKER_TWO = "DOCE_E2E_WORKSPACE_MARKER_TWO what's 2+2";

async function bubbleTexts(): Promise<string[]> {
  const bubbles = await browser.$$("[data-testid='chat-message']");
  const texts: string[] = [];
  for (let i = 0; i < bubbles.length; i++) {
    texts.push(await bubbles[i].getText());
  }
  return texts;
}

async function openEmptyState() {
  await browser.keys([Key.Command, "n"]);
  const input = await browser.$("[data-testid='empty-state-input']");
  await input.waitForExist({ timeout: 60000 });
  return input;
}

async function submitInitialWorkspaceTurn(text: string) {
  const input = await openEmptyState();
  await input.setValue(text);
  await (await browser.$("[data-testid='empty-state-submit']")).click();
  const agentInput = await browser.$("[data-testid='agent-input']");
  await agentInput.waitForExist({ timeout: 60000 });
}

async function waitForEditableWorkspaceInput() {
  const input = await browser.$("[data-testid='agent-input']");
  await input.waitForExist({ timeout: 60000 });
  await browser.waitUntil(async () => (await input.getAttribute("contenteditable")) === "true", {
    timeout: 60000,
    timeoutMsg: "workspace input never became editable",
  });
  return input;
}

async function waitForMessageFollowedByAnotherBubble(marker: string) {
  await browser.waitUntil(
    async () => {
      const texts = await bubbleTexts();
      const idx = texts.findIndex((t) => t.includes(marker));
      return idx !== -1 && idx + 1 < texts.length;
    },
    {
      timeout: 60000,
      timeoutMsg: `no response bubble appeared after ${marker}`,
    },
  );
}

describe("Workspace chat", () => {
  it("sends workspace turns from the empty state and keeps replies ordered", async () => {
    await submitInitialWorkspaceTurn(MARKER_ONE);
    await waitForMessageFollowedByAnotherBubble(MARKER_ONE);

    let texts = await bubbleTexts();
    const idxOne = texts.findIndex((t) => t.includes(MARKER_ONE));
    const nextBubble = texts[idxOne + 1];
    expect(nextBubble.trim().length).toBeGreaterThan(0);
    expect(nextBubble).not.toContain(MARKER_ONE);

    const input = await waitForEditableWorkspaceInput();
    await input.setValue(MARKER_TWO);
    await (await browser.$("[data-testid='agent-send']")).click();

    await waitForMessageFollowedByAnotherBubble(MARKER_TWO);

    texts = await bubbleTexts();
    const finalIdxOne = texts.findIndex((t) => t.includes(MARKER_ONE));
    const idxTwo = texts.findIndex((t) => t.includes(MARKER_TWO));
    expect(finalIdxOne).toBeGreaterThanOrEqual(0);
    expect(idxTwo).toBeGreaterThan(finalIdxOne);
    expect(texts[finalIdxOne + 1].trim().length).toBeGreaterThan(0);
    expect(texts[finalIdxOne + 1]).not.toContain(MARKER_ONE);
    expect(texts[finalIdxOne + 1]).not.toContain(MARKER_TWO);
    expect(texts[idxTwo + 1].trim().length).toBeGreaterThan(0);
    expect(texts[idxTwo + 1]).not.toContain(MARKER_TWO);
  });
});
