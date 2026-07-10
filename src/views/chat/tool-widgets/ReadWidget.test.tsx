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

    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read /tmp/notes.txt");
    expect(screen.getByText("11B")).toBeInTheDocument();
    expect(screen.getByText("312 tok")).toBeInTheDocument();
    expect(
      screen.getByTestId("read-widget").querySelector('[data-slot="widget-frame-chevron"]'),
    ).toBeInTheDocument();
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
    await userEvent.click(screen.getByRole("button"));

    const preview = screen.getByTestId("read-preview");
    expect(preview.querySelector(".max-h-80")).toBeInTheDocument();
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
    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read /tmp/big.txt");
    expect(screen.getByText("16B")).toBeInTheDocument();
    expect(screen.getByText("42 tok")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button"));
    expect(screen.queryByText("Output truncated")).not.toBeInTheDocument();
  });

  it("does not present offload as a separate summary-level state, but still offers the payload file once expanded (legacy row)", async () => {
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

    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read /tmp/huge.txt");
    expect(screen.getByText("15B")).toBeInTheDocument();
    expect(screen.getByText("2.0k tok")).toBeInTheDocument();
    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("read-text-preview")).toHaveTextContent("preview only...");
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
  });

  // --- Task 9 (payload-files design): slimmed detail shapes ---

  it("renders the content preview and offers the payload file (new row)", async () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/big.rs",
      offset: null,
      limit: null,
      payloadRef: "/tmp/big.rs",
      outcome: {
        ok: true,
        truncated: true,
        contentPreview: "pub fn execute(...",
        contentBytes: 48213,
      },
    };

    render(<ReadWidget detail={detail} />);
    await userEvent.click(screen.getByRole("button"));

    expect(screen.getByTestId("read-text-preview")).toHaveTextContent("pub fn execute(...");
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
  });

  it("still renders legacy rows with inline content (no contentPreview/payloadRef)", async () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };

    render(<ReadWidget detail={detail} />);
    await userEvent.click(screen.getByRole("button"));

    expect(screen.getByTestId("read-text-preview")).toHaveTextContent("hello world");
    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
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

    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read /tmp/legacy.txt");
    expect(screen.getByText("11B")).toBeInTheDocument();
    expect(screen.queryByText(/tok/)).not.toBeInTheDocument();
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

    expect(screen.getByRole("alert")).toHaveTextContent(/No such file or directory/);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("read-summary")).not.toBeInTheDocument();
  });
});
