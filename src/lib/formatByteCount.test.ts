import { describe, it, expect } from "vitest";
import { formatByteCount } from "./formatByteCount";

describe("formatByteCount", () => {
  it("shows the exact byte count under 1000 bytes", () => {
    expect(formatByteCount(0)).toBe("0B");
    expect(formatByteCount(42)).toBe("42B");
    expect(formatByteCount(999)).toBe("999B");
  });

  it("shows one decimal KB past 1000 bytes", () => {
    expect(formatByteCount(1000)).toBe("1.0KB");
    expect(formatByteCount(1500)).toBe("1.5KB");
    expect(formatByteCount(15600)).toBe("15.6KB");
  });
});
