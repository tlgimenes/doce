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

    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("search-summary")).toHaveTextContent("Found 2 files");
    expect(screen.queryByText("42 tok")).not.toBeInTheDocument();
    expect(screen.queryByTestId("search-match")).not.toBeInTheDocument();
    expect(screen.queryByTestId("search-results")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button"));

    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("search-results").querySelector(".max-h-80")).toBeInTheDocument();
    expect(screen.getAllByTestId("search-match")).toHaveLength(2);
    expect(screen.getByText("/tmp/project/a.rs")).toBeInTheDocument();
    // The pattern moves off the collapsed sentence into the expanded context.
    expect(screen.getByTestId("search-context")).toHaveTextContent("pattern: *.rs");
    expect(screen.getByTestId("search-context")).toHaveTextContent("/tmp/project");
    expect(screen.getByTestId("search-meta")).toHaveTextContent("42 tok");
  });

  it("renders a collapsible zero-files state for Glob", async () => {
    const detail: GlobDetail = { toolName: "Glob", pattern: "*.nope", path: "/tmp", matches: [] };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("No files matched");
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button"));

    const empty = screen.getByTestId("search-no-matches");
    expect(empty).toHaveTextContent("No files matched");
    expect(empty.closest('[data-slot="empty"]')).not.toBeNull();
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

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Found 1 match for TODO");
    expect(screen.queryByText("99 tok")).not.toBeInTheDocument();
    expect(screen.queryByTestId("search-match")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button"));

    const match = screen.getByTestId("search-match");
    expect(match).toHaveTextContent("/tmp/project/a.rs:12: // TODO: fix this");
    expect(screen.getByTestId("search-context")).toHaveTextContent("/tmp/project");
    expect(screen.getByTestId("search-context")).toHaveTextContent("*.rs");
    expect(screen.getByTestId("search-meta")).toHaveTextContent("99 tok");
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

    expect(screen.getByTestId("search-summary")).toHaveTextContent("No matches for nonexistent");
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button"));

    const empty = screen.getByTestId("search-no-matches");
    expect(empty).toHaveTextContent("No matches found");
    expect(empty.closest('[data-slot="empty"]')).not.toBeNull();
  });

  it("shows no token cost when tokenCount is absent", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs"],
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.queryByText(/tok/)).not.toBeInTheDocument();
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
    const badge = screen.getByTestId("search-widget").querySelector('[data-slot="badge"]');
    expect(badge).toHaveTextContent("Interrupted");
    expect(screen.queryByTestId("search-summary")).not.toBeInTheDocument();
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
