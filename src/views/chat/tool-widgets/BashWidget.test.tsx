import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import BashWidget from "./BashWidget";
import type { BashDetail } from "@/lib/ipc";

describe("BashWidget (004-tool-call-widgets, US2)", () => {
  it("renders the command and stdout/stderr together, monospaced, distinguishable from prose (FR-003)", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "ls -la",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "a.txt\nb.txt", stderr: "" },
    };
    render(<BashWidget detail={detail} />);

    expect(screen.getByTestId("bash-command")).toHaveTextContent("ls -la");
    expect(screen.getByTestId("bash-stdout")).toHaveTextContent("a.txt");
    expect(screen.getByTestId("bash-stdout")).toHaveTextContent("b.txt");
  });

  it("visually distinguishes success from failure via exitCode without requiring the output text to be read", () => {
    const success: BashDetail = {
      toolName: "Bash",
      command: "true",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "", stderr: "" },
    };
    const { rerender } = render(<BashWidget detail={success} />);
    expect(screen.getByTestId("bash-status")).toHaveTextContent(/success|0/i);

    const failure: BashDetail = {
      toolName: "Bash",
      command: "false",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 1, stdout: "", stderr: "boom" },
    };
    rerender(<BashWidget detail={failure} />);
    expect(screen.getByTestId("bash-status")).toHaveTextContent(/fail|1/i);
    expect(screen.getByTestId("bash-stderr")).toHaveTextContent("boom");
  });

  it("shows a dispatch-level failure (e.g. denylisted command) distinctly from a completed non-zero exit", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "rm -rf ~",
      timeoutMs: null,
      outcome: { ok: false, error: "blocked: catastrophic command" },
    };
    render(<BashWidget detail={detail} />);
    expect(screen.getByTestId("bash-status")).toBeInTheDocument();
    expect(screen.getByText(/catastrophic command/)).toBeInTheDocument();
  });

  it("truncates or collapses very long output rather than rendering it in full inline (FR-004)", () => {
    const longOutput = Array.from({ length: 500 }, (_, i) => `line ${i}`).join("\n");
    const detail: BashDetail = {
      toolName: "Bash",
      command: "seq 1 500",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: longOutput, stderr: "" },
    };
    render(<BashWidget detail={detail} />);

    const stdout = screen.getByTestId("bash-stdout");
    expect(stdout.textContent!.length).toBeLessThan(longOutput.length);
    expect(screen.getByTestId("bash-output-truncated")).toBeInTheDocument();
  });

  // --- 010-context-window-management/US3 ---

  it("shows a 'View full output' affordance when the result was offloaded", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "find / -name '*.log'",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "preview only...", stderr: "" },
      offloadedTo: "/data/tool-outputs/conv1/call1.txt",
    };
    render(<BashWidget detail={detail} />);
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
  });

  it("does not show the affordance when the result was not offloaded", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "echo hi",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "hi", stderr: "" },
      offloadedTo: null,
    };
    render(<BashWidget detail={detail} />);
    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
  });
});
