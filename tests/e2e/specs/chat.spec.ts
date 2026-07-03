import { expect } from "@wdio/globals";

// Covers quickstart.md §2 (User Story 2: chat): types real messages into the
// real chat UI, clicks send, and waits for real assistant responses to
// stream back — no mocked backend, no stubbed IPC. This exercises the full
// path: chat-input -> send_message command -> llama-cpp-2 inference on the
// real installed model -> assistant-token events -> Zustand stream store ->
// React render. Requires a model to already be installed (the onboarding
// spec's download, or a prior manual run, must have completed first) —
// this spec does not itself wait out a multi-GB download.
//
// Deliberately does NOT assert on the transient "Queued…"/"Generating…"/
// assistant-stream placeholder elements: on this model + hardware, a reply
// can complete in well under a second, so that placeholder can flash in and
// out of the DOM between two poll ticks. Asserting on it turned this into a
// flaky test without adding real coverage — the loading-state behavior
// itself is covered by a fast, deterministic frontend unit test instead
// (src/views/chat/Chat.test.tsx). This spec asserts on the stable, final,
// persisted message bubbles.
// Distinctive markers, not generic phrasing: `@wdio/tauri-service`'s
// embedded driver launches a real, visible, non-headless app window on this
// machine — an actual person can (and, during development, did) click into
// it and type something out of curiosity while a test was mid-flight. Using
// unmistakable markers and asserting on *relative* ordering around them
// (rather than an exact total bubble count) keeps these assertions correct
// regardless of any such incidental extra messages elsewhere in the list.
const MARKER_ONE = "DOCE_E2E_MARKER_ONE say hello in exactly three words";
const MARKER_TWO = "DOCE_E2E_MARKER_TWO what's 2+2";

async function bubbleTexts(): Promise<string[]> {
  const bubbles = await browser.$$("[data-testid='chat-message']");
  const texts: string[] = [];
  for (let i = 0; i < bubbles.length; i++) {
    texts.push(await bubbles[i].getText());
  }
  return texts;
}

describe("Chat (User Story 2: send a message, get a real response)", () => {
  it("sends a message and renders a real, non-empty assistant reply immediately after it", async () => {
    // 006-chat-empty-state: every conversation created through the UI is
    // now always workspace-scoped (agent mode) — this plain, non-agent
    // Chat.tsx/send_message path is only reachable for a conversation that
    // already existed before that feature shipped (FR-012's regression
    // guarantee), so one is seeded directly via the app's own real
    // create_conversation command (no workspaceId) rather than through a
    // UI path that no longer exists.
    await browser.execute(() => {
      return (
        window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args: unknown) => Promise<unknown> } }
      ).__TAURI_INTERNALS__.invoke("create_conversation", {});
    });

    const item = await browser.$("[data-testid='conversation-item']");
    await item.waitForExist({ timeout: 15000 });
    await item.click();

    const input = await browser.$("[data-testid='chat-input']");
    await input.waitForExist({ timeout: 15000 });
    await input.setValue(MARKER_ONE);
    await (await browser.$("[data-testid='chat-send']")).click();

    await browser.waitUntil(
      async () => {
        const texts = await bubbleTexts();
        const idx = texts.findIndex((t) => t.includes(MARKER_ONE));
        return idx !== -1 && idx + 1 < texts.length;
      },
      { timeout: 60000, timeoutMsg: "assistant reply never appeared as a persisted message bubble" },
    );

    const texts = await bubbleTexts();
    const idx = texts.findIndex((t) => t.includes(MARKER_ONE));
    const replyText = texts[idx + 1];
    expect(replyText.trim().length).toBeGreaterThan(0);
    expect(replyText).not.toContain(MARKER_ONE);
  });

  it("orders messages as real user/assistant turns, not all-user-then-reply", async () => {
    const input = await browser.$("[data-testid='chat-input']");
    await input.setValue(MARKER_TWO);
    await (await browser.$("[data-testid='chat-send']")).click();

    await browser.waitUntil(
      async () => {
        const texts = await bubbleTexts();
        const idx = texts.findIndex((t) => t.includes(MARKER_TWO));
        return idx !== -1 && idx + 1 < texts.length;
      },
      { timeout: 60000, timeoutMsg: "second assistant reply never appeared" },
    );

    const texts = await bubbleTexts();

    // The exact ordering bug reported: every user turn piled up before any
    // assistant turn rendered. Real back-and-forth means whatever
    // immediately follows each user message is its own reply — not another
    // user bubble (this is the invariant the bug broke; it's checked here
    // regardless of how many total bubbles exist).
    const idxOne = texts.findIndex((t) => t.includes(MARKER_ONE));
    const idxTwo = texts.findIndex((t) => t.includes(MARKER_TWO));
    expect(idxOne).toBeGreaterThanOrEqual(0);
    expect(idxTwo).toBeGreaterThan(idxOne);
    expect(texts[idxOne + 1]).not.toContain(MARKER_ONE);
    expect(texts[idxOne + 1]).not.toContain(MARKER_TWO);
    expect(texts[idxTwo + 1].trim().length).toBeGreaterThan(0);
    expect(texts[idxTwo + 1]).not.toContain(MARKER_TWO);
  });
});
