import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import EditDiffWidget from "./EditDiffWidget";
import type { EditDetail } from "@/lib/ipc";

describe("EditDiffWidget (004-tool-call-widgets, US1)", () => {
  it("renders a labeled diff with the file path and distinguishable added/removed lines (FR-002)", () => {
    const detail: EditDetail = {
      toolName: "Edit",
      filePath: "/tmp/notes.md",
      oldString: "line one\nold line\nline three",
      newString: "line one\nnew line\nline three",
      replaceAll: false,
      outcome: { ok: true },
    };

    render(<EditDiffWidget detail={detail} />);

    expect(screen.getByText("/tmp/notes.md")).toBeInTheDocument();
    const removed = screen.getByTestId("diff-removed");
    const added = screen.getByTestId("diff-added");
    expect(removed).toHaveTextContent("old line");
    expect(added).toHaveTextContent("new line");
    expect(removed.querySelector('[data-variant="removed"]')).not.toBeNull();
    expect(added.querySelector('[data-variant="added"]')).not.toBeNull();
  });

  it("shows the file path and +N/−N change-count badges in the header (FR-002)", () => {
    // oldString has one line replaced ("old line" -> "new line"): +1/-1.
    const detail: EditDetail = {
      toolName: "Edit",
      filePath: "/tmp/notes.md",
      oldString: "line one\nold line\nline three",
      newString: "line one\nnew line\nline three",
      replaceAll: false,
      outcome: { ok: true },
    };

    render(<EditDiffWidget detail={detail} />);

    expect(screen.getByText("+1")).toBeInTheDocument();
    expect(screen.getByText("−1")).toBeInTheDocument();
  });

  it("sums added/removed lines across multiple non-adjacent hunks in the +N/−N badges", () => {
    // Two separate single-line edits ("b"->"X", "d"->"Y") separated by
    // unchanged lines produce two added/removed hunks each — the badges
    // must sum across all of them, not just the first: +2/−2.
    const detail: EditDetail = {
      toolName: "Edit",
      filePath: "/tmp/multi.md",
      oldString: "a\nb\nc\nd\ne",
      newString: "a\nX\nc\nY\ne",
      replaceAll: false,
      outcome: { ok: true },
    };

    render(<EditDiffWidget detail={detail} />);

    expect(screen.getByText("+2")).toBeInTheDocument();
    expect(screen.getByText("−2")).toBeInTheDocument();
  });

  it("renders a failed-edit state, not an empty or misleading diff, when outcome.ok is false", () => {
    const detail: EditDetail = {
      toolName: "Edit",
      filePath: "/tmp/notes.md",
      oldString: "nonexistent",
      newString: "replacement",
      replaceAll: false,
      outcome: { ok: false, error: "no match found for the given old_string" },
    };

    render(<EditDiffWidget detail={detail} />);

    expect(screen.getByTestId("edit-failed")).toBeInTheDocument();
    expect(screen.getByText(/no match found/)).toBeInTheDocument();
    expect(screen.queryByTestId("diff-added")).not.toBeInTheDocument();
    expect(screen.queryByTestId("diff-removed")).not.toBeInTheDocument();
  });
});
