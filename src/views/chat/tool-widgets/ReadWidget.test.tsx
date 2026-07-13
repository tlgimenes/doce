import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import ReadWidget from "./ReadWidget";
import type { ReadDetail } from "@/lib/ipc";

describe("ReadWidget (004-tool-call-widgets, US4)", () => {
  it("renders a single quiet line: basename plus muted size info, nothing expandable", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read notes.txt");
    expect(screen.getByTestId("read-summary")).toHaveAttribute("title", "/tmp/notes.txt");
    expect(screen.getByTestId("read-meta")).toHaveTextContent("11B");
    // No accordion: no trigger button, no content panel.
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByText("hello world")).not.toBeInTheDocument();
  });

  it("does not present truncation or offload as visible states", () => {
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

    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read huge.txt");
    expect(screen.getByTestId("read-meta")).toHaveTextContent("15B");
    expect(screen.queryByText(/truncated/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/offload/i)).not.toBeInTheDocument();
  });

  it("uses contentBytes for new payload-files rows", () => {
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

    expect(screen.getByTestId("read-meta")).toHaveTextContent("48.2KB");
  });

  it("omits only the token segment for older rows without tokenCount", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/legacy.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read legacy.txt");
    expect(screen.getByTestId("read-meta")).toHaveTextContent("11B");
    expect(screen.queryByText(/tok/)).not.toBeInTheDocument();
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

    expect(screen.getByTestId("read-widget")).toHaveTextContent("Couldn't read missing.txt");
    expect(screen.getByTestId("read-widget")).toHaveTextContent(/No such file or directory/);
    expect(screen.queryByTestId("read-summary")).not.toBeInTheDocument();
  });
});
