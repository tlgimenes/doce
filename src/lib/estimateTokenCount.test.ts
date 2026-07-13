import { describe, expect, it } from "vitest";
import { estimateTokenCount } from "./estimateTokenCount";

describe("estimateTokenCount", () => {
  it("estimates roughly four characters per token, rounding up", () => {
    expect(estimateTokenCount("")).toBe(0);
    expect(estimateTokenCount("abcd")).toBe(1);
    expect(estimateTokenCount("abcde")).toBe(2);
    expect(estimateTokenCount("which folders do you see?")).toBe(7);
  });
});
