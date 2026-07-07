import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync } from "react-dom";
import { runViewTransition } from "./viewTransition";

vi.mock("react-dom", () => ({
  flushSync: vi.fn((callback: () => void) => callback()),
}));

type TestDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

const originalStartViewTransition = (document as TestDocument).startViewTransition;

afterEach(() => {
  Object.defineProperty(document, "startViewTransition", {
    configurable: true,
    value: originalStartViewTransition,
  });
  vi.clearAllMocks();
});

describe("runViewTransition", () => {
  it("updates immediately when the View Transition API is unavailable", () => {
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      value: undefined,
    });
    const update = vi.fn();

    runViewTransition(update);

    expect(update).toHaveBeenCalledTimes(1);
    expect(flushSync).not.toHaveBeenCalled();
  });

  it("uses startViewTransition and flushSync when supported", () => {
    const update = vi.fn();
    const startViewTransition = vi.fn((callback: () => void) => {
      callback();
      return {};
    });
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      value: startViewTransition,
    });

    runViewTransition(update);

    expect(startViewTransition).toHaveBeenCalledTimes(1);
    expect(flushSync).toHaveBeenCalledWith(update);
    expect(update).toHaveBeenCalledTimes(1);
  });

  it("falls back to one immediate update if starting the transition throws before update", () => {
    const update = vi.fn();
    Object.defineProperty(document, "startViewTransition", {
      configurable: true,
      value: vi.fn(() => {
        throw new Error("transition failed");
      }),
    });

    runViewTransition(update);

    expect(update).toHaveBeenCalledTimes(1);
  });
});
