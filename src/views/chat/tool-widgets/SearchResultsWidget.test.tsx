import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import SearchResultsWidget from "./SearchResultsWidget";
import type { GlobDetail, GrepDetail } from "@/lib/ipc";

describe("SearchResultsWidget (004-tool-call-widgets, US4: Glob + Grep)", () => {
  it("renders a Glob match list, not a raw dump (FR-007)", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs", "/tmp/project/b.rs"],
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getAllByTestId("search-match")).toHaveLength(2);
    expect(screen.getByText("/tmp/project/a.rs")).toBeInTheDocument();
  });

  it("renders a legible zero-matches state for Glob", () => {
    const detail: GlobDetail = { toolName: "Glob", pattern: "*.nope", path: "/tmp", matches: [] };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-no-matches")).toBeInTheDocument();
  });

  it("renders a Grep match list with file, line number, and line content", () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "TODO",
      path: "/tmp/project",
      glob: null,
      matches: [{ path: "/tmp/project/a.rs", lineNumber: 12, line: "// TODO: fix this" }],
    };
    render(<SearchResultsWidget detail={detail} />);
    const match = screen.getByTestId("search-match");
    expect(match).toHaveTextContent("a.rs");
    expect(match).toHaveTextContent("12");
    expect(match).toHaveTextContent("TODO: fix this");
  });

  it("renders a legible zero-matches state for Grep", () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "nonexistent",
      path: "/tmp",
      glob: null,
      matches: [],
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-no-matches")).toBeInTheDocument();
  });
});
