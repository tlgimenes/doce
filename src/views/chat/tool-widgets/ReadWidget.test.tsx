import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import ReadWidget from "./ReadWidget";
import type { ReadDetail } from "@/lib/ipc";

describe("ReadWidget (004-tool-call-widgets, US4)", () => {
  it("renders a compact successful file-reference card with path, bytes, and tokens", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toBeInTheDocument();
    expect(screen.getByTestId("read-widget")).toHaveTextContent(
      "Read /tmp/notes.txt · 11B · 312 tok",
    );
  });

  it("does not present truncation as a separate visible state", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/big.txt",
      offset: null,
      limit: 2000,
      outcome: { ok: true, content: "a lot of content", truncated: true },
      tokenCount: 42,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.queryByTestId("read-truncated")).not.toBeInTheDocument();
    expect(screen.queryByText("Output truncated")).not.toBeInTheDocument();
    expect(screen.getByTestId("read-widget")).toHaveTextContent(
      "Read /tmp/big.txt · 16B · 42 tok",
    );
  });

  it("renders byte metadata and omits only the token segment for older rows without tokenCount", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/legacy.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toHaveTextContent("Read /tmp/legacy.txt · 11B");
    expect(screen.getByTestId("read-widget")).not.toHaveTextContent("tok");
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

    expect(screen.getByTestId("read-widget")).toHaveClass("border-destructive/40");
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
      tokenCount: 2048,
      offloadedTo: "/data/tool-outputs/conv1/call1.txt",
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toHaveTextContent(
      "Read /tmp/huge.txt · 15B · 2.0k tok",
    );
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
    expect(screen.queryByTestId("read-truncated")).not.toBeInTheDocument();
  });

  it("does not show the full-output affordance when the result was not offloaded", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
      offloadedTo: null,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
  });
});
