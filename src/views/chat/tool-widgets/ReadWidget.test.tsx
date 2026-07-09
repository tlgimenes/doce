import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ReadWidget from "./ReadWidget";
import type { ReadDetail } from "@/lib/ipc";

describe("ReadWidget (004-tool-call-widgets, US4)", () => {
  it("renders successful reads collapsed with path, bytes, tokens, and a chevron", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).not.toHaveAttribute("open");
    expect(screen.getByTestId("read-summary")).toHaveTextContent(
      "Read /tmp/notes.txt · 11B · 312 tok",
    );
    expect(screen.getByTestId("tool-disclosure-chevron")).toBeInTheDocument();
    expect(screen.queryByTestId("read-preview")).not.toBeInTheDocument();
  });

  it("expands inline to show captured text preview", async () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "captured text", truncated: false },
      tokenCount: 20,
    };

    render(<ReadWidget detail={detail} />);
    await userEvent.click(screen.getByTestId("read-summary"));

    expect(screen.getByTestId("read-preview")).toHaveClass("max-h-80");
    expect(screen.getByTestId("read-text-preview")).toHaveTextContent("captured text");
  });

  it("does not present truncation as a separate visible state", async () => {
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
    expect(screen.getByTestId("read-summary")).toHaveTextContent(
      "Read /tmp/big.txt · 16B · 42 tok",
    );
    await userEvent.click(screen.getByTestId("read-summary"));
    expect(screen.queryByText("Output truncated")).not.toBeInTheDocument();
  });

  it("does not present offload as a separate visible state", () => {
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

    expect(screen.getByTestId("read-summary")).toHaveTextContent(
      "Read /tmp/huge.txt · 15B · 2.0k tok",
    );
    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
    expect(screen.queryByText("View full output")).not.toBeInTheDocument();
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

    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read /tmp/legacy.txt · 11B");
    expect(screen.getByTestId("read-summary")).not.toHaveTextContent("tok");
  });

  it("renders a failure state distinctly and not as a disclosure", () => {
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
    expect(screen.queryByTestId("read-summary")).not.toBeInTheDocument();
  });
});
