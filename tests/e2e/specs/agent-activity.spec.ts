import { Key } from "webdriverio";

// E2E coverage for the consolidated agent-activity status line: the single
// strip docked above the composer (goal/current-todo primary · progress ·
// working), driven here from the WidgetGallery's mock snapshots so every
// state is reachable without a live turn. Verifies the cards render, the
// primary slot truncates on one line, and the ⌄ expands the plan checklist.
// Gated to workflow_dispatch like the rest of the suite (needs the built app).

async function truncation(scope: WebdriverIO.Element, selector: string) {
  const span = await scope.$(selector);
  await span.waitForExist({ timeout: 5000 });
  return browser.execute(
    (el) => ({
      scrollWidth: (el as HTMLElement).scrollWidth,
      clientWidth: (el as HTMLElement).clientWidth,
      overflow: getComputedStyle(el as HTMLElement).overflow,
      textOverflow: getComputedStyle(el as HTMLElement).textOverflow,
    }),
    span as unknown as HTMLElement,
  );
}

describe("Agent activity status line", () => {
  it("renders the gallery cards, truncates the primary slot on one line, and expands the plan", async () => {
    await browser.setWindowSize(1200, 800);

    await browser.waitUntil(
      async () =>
        (await browser.$("[data-testid='conversation-list']").isExisting()) ||
        (await browser.$("[data-testid='empty-state-input']").isExisting()),
      { timeout: 60000, timeoutMsg: "app never became ready" },
    );

    // Open the widget gallery (Cmd+D).
    await browser.keys([Key.Command, "d"]);
    await (await browser.$("[data-testid='widget-gallery']")).waitForExist({ timeout: 10000 });

    // The "Agent activity" section renders four AgentActivityView cards.
    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='agent-activity']")).length >= 4,
      { timeout: 10000, timeoutMsg: "agent-activity cards never rendered" },
    );

    // The "no goal -> current todo" card: its primary slot shows the current
    // todo, which is long and must truncate with an ellipsis on one line.
    const cards = await browser.$$("[data-testid='agent-activity']");
    let todoCard: WebdriverIO.Element | undefined;
    for (const card of cards) {
      if (await card.$("[data-testid='agent-activity-current-todo']").isExisting()) {
        todoCard = card;
        break;
      }
    }
    if (!todoCard) throw new Error("no card with a current-todo primary slot found");
    await todoCard.scrollIntoView({ block: "center" });

    const primary = await truncation(todoCard, "[data-testid='agent-activity-current-todo']");
    if (primary.scrollWidth <= primary.clientWidth) {
      throw new Error(
        `primary slot does not overflow (scroll ${primary.scrollWidth} <= client ${primary.clientWidth}) — truncation not exercised`,
      );
    }
    if (primary.overflow !== "hidden" || primary.textOverflow !== "ellipsis") {
      throw new Error(`primary slot not truncating: ${JSON.stringify(primary)}`);
    }

    // The whole pill must sit on one line.
    const pill = await todoCard.$("[data-testid='plan-tracker']");
    const pillHeight = (await pill.getSize()).height;
    if (pillHeight > 48) throw new Error(`pill wrapped to multiple lines (height ${pillHeight}px)`);

    // Expand: the plan checklist appears; the long step row also truncates.
    await (await todoCard.$("[data-testid='agent-activity-expander']")).click();
    await browser.waitUntil(
      async () => (await todoCard!.$$("[data-testid='plan-step']")).length === 3,
      { timeout: 5000, timeoutMsg: "expanded plan checklist never rendered" },
    );

    const steps = await todoCard.$$("[data-testid='plan-step']");
    const longStep = steps[1];
    const stepTrunc = await truncation(longStep, "span.truncate");
    if (stepTrunc.scrollWidth <= stepTrunc.clientWidth) {
      throw new Error(
        `long step does not overflow (scroll ${stepTrunc.scrollWidth} <= client ${stepTrunc.clientWidth})`,
      );
    }
    if (stepTrunc.overflow !== "hidden" || stepTrunc.textOverflow !== "ellipsis") {
      throw new Error(`long step not truncating: ${JSON.stringify(stepTrunc)}`);
    }
  });
});
