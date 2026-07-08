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

  // --- 010-context-window-management/US3 ---

  it("shows a 'View full output' affordance when the result was offloaded", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/huge.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "preview only...", truncated: true },
      offloadedTo: "/data/tool-outputs/conv1/call1.txt",
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
  });

  it("does not show the affordance when the result was not offloaded", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      offloadedTo: null,
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
  });

  it("shows a byte/token cost badge when tokenCount is present", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByTestId("read-widget")).toHaveTextContent("312 tok");
  });

  it("shows no cost badge when tokenCount is absent", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };
    render(<ReadWidget detail={detail} />);
    expect(screen.getByTestId("read-widget")).not.toHaveTextContent("tok");
  });
});
