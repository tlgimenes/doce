import { Key } from "webdriverio";

// Temp measurement spec (uncommitted): pixel-level check of the plan
// tracker truncation in the gallery — card width vs max-w-xl (576px),
// page horizontal overflow, span clip widths.
const SHOT_DIR =
  "/private/tmp/claude-501/-Users-gimenes-code-doce/2b4b842f-6fd7-4591-9091-49c530ce299b/scratchpad";

describe("Plan tracker truncation measurements", () => {
  it("measures the long-text card in collapsed and expanded states", async () => {
    await browser.setWindowSize(1200, 800);
    await browser.waitUntil(
      async () =>
        (await browser.$("[data-testid='conversation-list']").isExisting()) ||
        (await browser.$("[data-testid='empty-state-input']").isExisting()),
      { timeout: 60000, timeoutMsg: "app never became ready" },
    );

    await browser.keys([Key.Command, "d"]);
    await browser.$("[data-testid='widget-gallery']").waitForExist({ timeout: 10000 });
    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='plan-tracker']")).length >= 4,
      { timeout: 10000, timeoutMsg: "plan tracker mocks never rendered" },
    );

    const metrics = await browser.execute(() => {
      const gallery = document.querySelector("[data-testid='widget-gallery']") as HTMLElement;
      const cards = Array.from(
        document.querySelectorAll("[data-testid='plan-tracker']"),
      ) as HTMLElement[];
      const card = cards[1];
      const trigger = card.querySelector("[data-testid='plan-current-step']") as HTMLElement;
      const span = trigger.querySelector("span.min-w-0.truncate") as HTMLElement;
      const cs = getComputedStyle(span);
      return {
        windowInnerWidth: window.innerWidth,
        galleryScrollWidth: gallery.scrollWidth,
        galleryClientWidth: gallery.clientWidth,
        docScrollWidth: document.documentElement.scrollWidth,
        cardWidth: card.getBoundingClientRect().width,
        triggerWidth: trigger.getBoundingClientRect().width,
        triggerHeight: trigger.getBoundingClientRect().height,
        spanClientWidth: span.clientWidth,
        spanScrollWidth: span.scrollWidth,
        spanOverflow: cs.overflow,
        spanTextOverflow: cs.textOverflow,
        spanWhiteSpace: cs.whiteSpace,
      };
    });
    console.log("METRICS_COLLAPSED " + JSON.stringify(metrics));

    const cards = await browser.$$("[data-testid='plan-tracker']");
    const longCard = cards[1];
    await longCard.scrollIntoView({ block: "center" });
    await longCard.saveScreenshot(`${SHOT_DIR}/card-collapsed.png`);

    await (await longCard.$("[data-testid='plan-current-step']")).click();
    await browser.waitUntil(
      async () => (await longCard.$$("[data-testid='plan-step']")).length === 3,
      { timeout: 5000, timeoutMsg: "expanded step list never rendered" },
    );

    const expandedMetrics = await browser.execute(() => {
      const cards = Array.from(
        document.querySelectorAll("[data-testid='plan-tracker']"),
      ) as HTMLElement[];
      const card = cards[1];
      const steps = Array.from(
        card.querySelectorAll("[data-testid='plan-step']"),
      ) as HTMLElement[];
      const longStep = steps[1];
      const span = longStep.querySelector("span.min-w-0.truncate") as HTMLElement;
      return {
        cardWidth: card.getBoundingClientRect().width,
        stepWidth: longStep.getBoundingClientRect().width,
        stepHeight: longStep.getBoundingClientRect().height,
        spanClientWidth: span.clientWidth,
        spanScrollWidth: span.scrollWidth,
        docScrollWidth: document.documentElement.scrollWidth,
      };
    });
    console.log("METRICS_EXPANDED " + JSON.stringify(expandedMetrics));

    await longCard.scrollIntoView({ block: "center" });
    await longCard.saveScreenshot(`${SHOT_DIR}/card-expanded.png`);
  });
});
