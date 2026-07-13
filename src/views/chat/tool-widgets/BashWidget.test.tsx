import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import BashWidget from "./BashWidget";
import type { BashDetail } from "@/lib/ipc";

describe("BashWidget (004-tool-call-widgets, US2)", () => {
  it("renders a completed command as a single line with muted exit/token info, no output", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "ls -la",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "a.txt\nb.txt", stderr: "" },
      tokenCount: 89,
    };
    render(<BashWidget detail={detail} />);

    expect(screen.getByTestId("bash-command")).toHaveTextContent("$ ls -la");
    expect(screen.getByTestId("bash-meta")).toHaveTextContent("exit 0 · 89 tok");
    // No accordion, no output in the transcript.
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByText(/a\.txt/)).not.toBeInTheDocument();
    // Success is the quiet default — no badge.
    expect(screen.getByTestId("bash-widget").querySelector('[data-slot="badge"]')).toBeNull();
  });

  it("omits the token segment when tokenCount is absent", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "true",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "", stderr: "" },
    };
    render(<BashWidget detail={detail} />);
    expect(screen.getByTestId("bash-meta")).toHaveTextContent("exit 0");
    expect(screen.queryByText(/tok/)).not.toBeInTheDocument();
  });

  it("visually distinguishes a completed non-zero exit by painting the exit segment in danger", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "false",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 1, stdout: "", stderr: "boom" },
    };
    render(<BashWidget detail={detail} />);
    // No badge, no "failed" note — the red exit code carries the state.
    expect(screen.getByTestId("bash-exit")).toHaveTextContent("exit 1");
    expect(screen.getByTestId("bash-exit")).toHaveClass("text-destructive");
    expect(screen.getByTestId("bash-widget").querySelector('[data-slot="badge"]')).toBeNull();
  });

  it("shows a dispatch-level rejection (e.g. denylisted command) as an amber denied note with the reason in a tooltip", async () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "rm -rf ~",
      timeoutMs: null,
      outcome: { ok: false, error: "command rejected: matches a catastrophic pattern" },
    };
    render(<BashWidget detail={detail} />);

    expect(screen.getByTestId("bash-command")).toHaveTextContent("$ rm -rf ~");
    const denied = screen.getByTestId("bash-denied");
    expect(denied).toHaveTextContent("denied");
    expect(denied).toHaveClass("text-amber-600");
    // The rejection reason is tucked into the tooltip, not the row.
    expect(screen.queryByText(/catastrophic pattern/)).not.toBeInTheDocument();
    await userEvent.hover(denied);
    expect(
      await screen.findByText("command rejected: matches a catastrophic pattern"),
    ).toBeInTheDocument();
    // A command that never ran has no exit code to report.
    expect(screen.queryByTestId("bash-meta")).not.toBeInTheDocument();
    expect(screen.getByTestId("bash-widget").querySelector('[data-slot="badge"]')).toBeNull();
  });

  // --- pending/running state (no outcome yet) ---

  it("renders a pending state (no outcome) as the shimmering command itself", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "cargo test --test agent_benchmark tier4_planned",
      timeoutMs: null,
    };
    render(<BashWidget detail={detail} />);
    // The shimmering command IS the running signal — no "Running…" copy,
    // no spinner icon, no expandable panel.
    expect(screen.getByTestId("bash-status")).toHaveTextContent(
      "$ cargo test --test agent_benchmark tier4_planned",
    );
    expect(screen.getByTestId("bash-status")).toHaveClass("shimmer");
    expect(screen.getByTestId("bash-widget").querySelector('[data-slot="spinner"]')).toBeNull();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("bash-meta")).not.toBeInTheDocument();
  });
});
