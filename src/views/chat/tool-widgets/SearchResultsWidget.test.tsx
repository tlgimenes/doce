import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import SearchResultsWidget from "./SearchResultsWidget";
import type { GlobDetail, GrepDetail } from "@/lib/ipc";

describe("SearchResultsWidget (004-tool-call-widgets, US4: Glob + Grep)", () => {
  it("renders Glob as a single outcome sentence with muted token info, no match list", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs", "/tmp/project/b.rs"],
      tokenCount: 42,
    };

    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Found 2 files");
    // The pattern lives in the hover title, not the sentence.
    expect(screen.getByTestId("search-summary")).toHaveAttribute("title", "*.rs");
    expect(screen.getByTestId("search-meta")).toHaveTextContent("42 tok");
    // No accordion, no match list in the transcript.
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByText("/tmp/project/a.rs")).not.toBeInTheDocument();
  });

  it("renders a zero-files sentence for Glob", () => {
    const detail: GlobDetail = { toolName: "Glob", pattern: "*.nope", path: "/tmp", matches: [] };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("No files matched");
  });

  it("renders Grep as an outcome sentence with the pattern inline", () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "TODO",
      path: "/tmp/project",
      glob: "*.rs",
      matches: [{ path: "/tmp/project/a.rs", lineNumber: 12, line: "// TODO: fix this" }],
      tokenCount: 99,
    };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Found 1 match for TODO");
    expect(screen.getByTestId("search-meta")).toHaveTextContent("99 tok");
    expect(screen.queryByText(/fix this/)).not.toBeInTheDocument();
  });

  it("renders a zero-matches sentence for Grep", () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "nonexistent",
      path: "/tmp",
      glob: null,
      matches: [],
    };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("No matches for nonexistent");
  });

  it("shows no token info when tokenCount is absent", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs"],
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.queryByTestId("search-meta")).not.toBeInTheDocument();
    expect(screen.queryByText(/tok/)).not.toBeInTheDocument();
  });

  it("renders an interrupted notice for a healed crash-orphaned Grep", () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "needle",
      path: "/tmp/project",
      glob: null,
      matches: [],
      interrupted: true,
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-interrupted")).toHaveTextContent(/interrupted/i);
    const badge = screen.getByTestId("search-widget").querySelector('[data-slot="badge"]');
    expect(badge).toHaveTextContent("Interrupted");
    expect(screen.queryByTestId("search-summary")).not.toBeInTheDocument();
  });
});
