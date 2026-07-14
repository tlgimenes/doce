import { Key } from "webdriverio";

// Temp verification spec (uncommitted): visual check of the todo/plan
// tracker truncation fix + Item primitive regression eyeball.
// Screenshots land in the session scratchpad.
const SHOT_DIR =
  "/private/tmp/claude-501/-Users-gimenes-code-doce/2b4b842f-6fd7-4591-9091-49c530ce299b/scratchpad";

async function overflows(selectorScope: WebdriverIO.Element, spanSelector: string) {
  const span = await selectorScope.$(spanSelector);
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

describe("Todo list truncation verification", () => {
  it("long step text truncates with an ellipsis in the plan tracker gallery", async () => {
    await browser.setWindowSize(1200, 800);

    // App ready = the empty-state or workspace surface exists.
    await browser.waitUntil(
      async () =>
        (await browser.$("[data-testid='conversation-list']").isExisting()) ||
        (await browser.$("[data-testid='empty-state-input']").isExisting()),
      { timeout: 60000, timeoutMsg: "app never became ready" },
    );

    // Open the widget gallery.
    await browser.keys([Key.Command, "d"]);
    const gallery = await browser.$("[data-testid='widget-gallery']");
    await gallery.waitForExist({ timeout: 10000 });

    // Four PlanTrackerCard mocks; index 1 is "Long step text (truncated with ellipsis)".
    await browser.waitUntil(
      async () => (await browser.$$("[data-testid='plan-tracker']")).length >= 4,
      { timeout: 10000, timeoutMsg: "plan tracker mocks never rendered" },
    );
    const cards = await browser.$$("[data-testid='plan-tracker']");
    const longCard = cards[1];
    await longCard.scrollIntoView({ block: "center" });

    // Collapsed: goal/current-step one-liner. currentStepIndex=1 → the long
    // description IS the collapsed line.
    const trigger = await longCard.$("[data-testid='plan-current-step']");
    const collapsed = await overflows(trigger, "span.min-w-0.truncate");
    console.log("COLLAPSED SPAN:", JSON.stringify(collapsed));

    // The badge + chevron must sit on the same line: single-line trigger height.
    const triggerHeight = (await trigger.getSize()).height;
    console.log("TRIGGER HEIGHT:", triggerHeight);

    await browser.saveScreenshot(`${SHOT_DIR}/plan-tracker-collapsed.png`);

    if (collapsed.scrollWidth <= collapsed.clientWidth) {
      throw new Error(
        `collapsed span does not overflow (scroll ${collapsed.scrollWidth} <= client ${collapsed.clientWidth}) — truncation not exercised`,
      );
    }
    if (collapsed.overflow !== "hidden" || collapsed.textOverflow !== "ellipsis") {
      throw new Error(`collapsed span not truncating: ${JSON.stringify(collapsed)}`);
    }
    if (triggerHeight > 48) {
      throw new Error(`trigger row wrapped to multiple lines (height ${triggerHeight}px)`);
    }

    // Expand and check the long step row.
    await trigger.click();
    await browser.waitUntil(async () => (await longCard.$$("[data-testid='plan-step']")).length === 3, {
      timeout: 5000,
      timeoutMsg: "expanded step list never rendered",
    });
    const steps = await longCard.$$("[data-testid='plan-step']");
    const longStep = steps[1];
    const expanded = await overflows(longStep, "span.min-w-0.truncate");
    console.log("EXPANDED SPAN:", JSON.stringify(expanded));
    const stepHeight = (await longStep.getSize()).height;
    console.log("STEP HEIGHT:", stepHeight);

    await longCard.scrollIntoView({ block: "center" });
    await browser.saveScreenshot(`${SHOT_DIR}/plan-tracker-expanded.png`);

    if (expanded.scrollWidth <= expanded.clientWidth) {
      throw new Error(
        `expanded span does not overflow (scroll ${expanded.scrollWidth} <= client ${expanded.clientWidth})`,
      );
    }
    if (expanded.overflow !== "hidden" || expanded.textOverflow !== "ellipsis") {
      throw new Error(`expanded span not truncating: ${JSON.stringify(expanded)}`);
    }
    if (stepHeight > 48) {
      throw new Error(`long step row wrapped to multiple lines (height ${stepHeight}px)`);
    }
  });

  it("Settings and topbar regression eyeball after the Item primitive change", async () => {
    // Close the gallery, open Settings via the sidebar action.
    await browser.keys([Key.Command, "d"]);
    const openSettings = await browser.$("[data-testid='open-settings']");
    await openSettings.waitForExist({ timeout: 10000 });
    await openSettings.click();
    const settings = await browser.$("[data-testid='settings-view']");
    await settings.waitForExist({ timeout: 10000 });
    const modelPanel = await browser.$("[data-testid='settings-model-panel']");
    await modelPanel.waitForExist({ timeout: 10000 });
    await browser.saveScreenshot(`${SHOT_DIR}/settings-regression.png`);
    await (await browser.$("[data-testid='close-settings']")).click();

    // Topbar: select an existing conversation if the real sidebar has one.
    const items = await browser.$$("[data-testid='conversation-thread-button']");
    console.log("CONVERSATION COUNT:", items.length);
    if (items.length > 0) {
      await items[0].click();
      const title = await browser.$("[data-testid='workspace-topbar-title']");
      await title.waitForExist({ timeout: 15000 });
      await browser.saveScreenshot(`${SHOT_DIR}/topbar-regression.png`);
    } else {
      console.log("NO EXISTING CONVERSATIONS — topbar check skipped");
    }
  });
});
