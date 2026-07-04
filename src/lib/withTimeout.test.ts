import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { withTimeout } from "./withTimeout";

describe("withTimeout", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("resolves with the promise's value when it settles before the timeout", async () => {
    const promise = withTimeout(Promise.resolve("value"), 1000);
    await vi.advanceTimersByTimeAsync(0);
    await expect(promise).resolves.toBe("value");
  });

  it("rejects with the promise's error when it rejects before the timeout", async () => {
    const rejected = Promise.reject(new Error("boom"));
    rejected.catch(() => {}); // mark handled before withTimeout's own .then() attaches
    const promise = withTimeout(rejected, 1000);
    const assertion = expect(promise).rejects.toThrow("boom");
    await vi.advanceTimersByTimeAsync(0);
    await assertion;
  });

  it("rejects once the timeout elapses if the promise never settles", async () => {
    const neverSettles = new Promise<string>(() => {});
    const promise = withTimeout(neverSettles, 1000, "gave up waiting");
    const assertion = expect(promise).rejects.toThrow("gave up waiting");
    await vi.advanceTimersByTimeAsync(1000);
    await assertion;
  });

  it("does not reject after the timeout if the promise already settled", async () => {
    const promise = withTimeout(Promise.resolve("fast"), 1000);
    await vi.advanceTimersByTimeAsync(0);
    await expect(promise).resolves.toBe("fast");
    // Letting the timer's own deadline pass afterward must not somehow
    // flip an already-resolved promise or throw an unhandled rejection.
    await vi.advanceTimersByTimeAsync(1000);
  });
});
