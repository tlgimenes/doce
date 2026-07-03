import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import ReadWidget from "./ReadWidget";
import type { ReadDetail } from "@/lib/ipc";

describe("ReadWidget (004-tool-call-widgets, US4)", () => {
  it("renders a compact file-reference card with at minimum the file path (FR-005)", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByTestId("read-widget")).toBeInTheDocument();
    expect(screen.getByText("/tmp/notes.txt")).toBeInTheDocument();
  });

  it("indicates truncation when the read result was capped", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/big.txt",
      offset: null,
      limit: 2000,
      outcome: { ok: true, content: "a lot of content", truncated: true },
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByTestId("read-truncated")).toBeInTheDocument();
  });

  it("renders a failure state distinctly", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/missing.txt",
      offset: null,
      limit: null,
      outcome: { ok: false, error: "No such file or directory" },
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByText(/No such file or directory/)).toBeInTheDocument();
  });
});
