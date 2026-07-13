import { describe, expect, it } from "vitest";
import { pathBasename } from "./pathBasename";

describe("pathBasename", () => {
  it("returns the last segment of an absolute path", () => {
    expect(pathBasename("/Users/tester/code/doce/src/App.tsx")).toBe("App.tsx");
  });

  it("ignores trailing slashes", () => {
    expect(pathBasename("/Users/tester/code/doce/")).toBe("doce");
  });

  it("returns a bare filename unchanged", () => {
    expect(pathBasename("notes.md")).toBe("notes.md");
  });

  it("falls back to the input for the filesystem root", () => {
    expect(pathBasename("/")).toBe("/");
  });
});
