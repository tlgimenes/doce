import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import SearchResultsWidget from "./SearchResultsWidget";
import type { GlobDetail, GrepDetail } from "@/lib/ipc";

describe("SearchResultsWidget (004-tool-call-widgets, US4: Glob + Grep)", () => {
  it("renders Glob collapsed with file count and expands to show file list", async () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs", "/tmp/project/b.rs"],
      tokenCount: 42,
    };

    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-widget")).not.toHaveAttribute("open");
    expect(screen.getByTestId("search-summary")).toHaveTextContent("Glob *.rs · 2 files · 42 tok");
    expect(screen.queryByTestId("search-match")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    expect(screen.getByTestId("search-results")).toHaveClass("max-h-80");
    expect(screen.getAllByTestId("search-match")).toHaveLength(2);
    expect(screen.getByText("/tmp/project/a.rs")).toBeInTheDocument();
    expect(screen.getByTestId("search-context")).toHaveTextContent("/tmp/project");
  });

  it("renders a collapsible zero-files state for Glob", async () => {
    const detail: GlobDetail = { toolName: "Glob", pattern: "*.nope", path: "/tmp", matches: [] };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Glob *.nope · 0 files");
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    expect(screen.getByTestId("search-no-matches")).toHaveTextContent("No files matched");
  });

  it("renders Grep collapsed with match count and expands to show match list", async () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "TODO",
      path: "/tmp/project",
      glob: "*.rs",
      matches: [{ path: "/tmp/project/a.rs", lineNumber: 12, line: "// TODO: fix this" }],
      tokenCount: 99,
    };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Grep TODO · 1 match · 99 tok");
    expect(screen.queryByTestId("search-match")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    const match = screen.getByTestId("search-match");
    expect(match).toHaveTextContent("/tmp/project/a.rs:12: // TODO: fix this");
    expect(screen.getByTestId("search-context")).toHaveTextContent("/tmp/project");
    expect(screen.getByTestId("search-context")).toHaveTextContent("*.rs");
  });

  it("renders a collapsible zero-matches state for Grep", async () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "nonexistent",
      path: "/tmp",
      glob: null,
      matches: [],
    };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Grep nonexistent · 0 matches");
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    expect(screen.getByTestId("search-no-matches")).toHaveTextContent("No matches found");
  });

  it("shows no token cost when tokenCount is absent", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs"],
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-summary")).not.toHaveTextContent("tok");
  });

  it("renders an interrupted notice — never a collapsed zero-result disclosure — for a healed crash-orphaned Grep", () => {
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
    expect(screen.queryByTestId("search-summary")).not.toBeInTheDocument();
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();
  });
});
