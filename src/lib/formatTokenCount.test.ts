import { describe, it, expect } from "vitest";
import { formatTokenCount } from "./formatTokenCount";

describe("formatTokenCount", () => {
  it("shows small counts exactly", () => {
    expect(formatTokenCount(0)).toBe("0");
    expect(formatTokenCount(42)).toBe("42");
    expect(formatTokenCount(999)).toBe("999");
  });

  it("abbreviates counts of 1000 or more with a 'k' suffix, one decimal place", () => {
    expect(formatTokenCount(1000)).toBe("1.0k");
    expect(formatTokenCount(1500)).toBe("1.5k");
    expect(formatTokenCount(15600)).toBe("15.6k");
    expect(formatTokenCount(123456)).toBe("123.5k");
  });
});
