import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import WriteWidget from "./WriteWidget";
import ReadWidget from "./ReadWidget";
import type { WriteDetail, ReadDetail } from "@/lib/ipc";

describe("WriteWidget (004-tool-call-widgets, US4)", () => {
  it("renders a compact file-reference card distinct from a plain reply (FR-006)", () => {
    const detail: WriteDetail = {
      toolName: "Write",
      filePath: "/tmp/new-file.txt",
      contentPreview: "hello world",
      byteCount: 11,
      outcome: { ok: true },
    };
    render(<WriteWidget detail={detail} />);
    expect(screen.getByTestId("write-widget")).toBeInTheDocument();
    expect(screen.getByText("/tmp/new-file.txt")).toBeInTheDocument();
  });

  it("renders the file path in an edit-like compact header with write metadata below it", () => {
    const detail: WriteDetail = {
      toolName: "Write",
      filePath: "/tmp/new-file.txt",
      contentPreview: "hello world",
      byteCount: 11,
      outcome: { ok: true },
    };

    render(<WriteWidget detail={detail} />);

    expect(screen.getByTestId("write-widget")).toHaveClass("overflow-hidden");
    expect(screen.getByTestId("write-header")).toHaveTextContent("/tmp/new-file.txt");
    expect(screen.getByTestId("write-header")).toHaveClass("px-3", "py-1.5");
    expect(screen.getByTestId("write-body")).toHaveTextContent("Write · 11 bytes");
  });

  it("renders a failure state distinctly", () => {
    const detail: WriteDetail = {
      toolName: "Write",
      filePath: "/root/no-permission.txt",
      contentPreview: "x",
      byteCount: 1,
      outcome: { ok: false, error: "permission denied" },
    };
    render(<WriteWidget detail={detail} />);
    expect(screen.getByText(/permission denied/)).toBeInTheDocument();
  });

  it("is visually distinguishable from ReadWidget for the same file path (FR-006)", () => {
    const writeDetail: WriteDetail = {
      toolName: "Write",
      filePath: "/tmp/f.txt",
      contentPreview: "x",
      byteCount: 1,
      outcome: { ok: true },
    };
    const readDetail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/f.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "x", truncated: false },
    };
    const { container: writeContainer } = render(<WriteWidget detail={writeDetail} />);
    const { container: readContainer } = render(<ReadWidget detail={readDetail} />);
    expect(writeContainer.querySelector('[data-testid="write-widget"]')).toBeInTheDocument();
    expect(readContainer.querySelector('[data-testid="read-widget"]')).toBeInTheDocument();
  });
});
