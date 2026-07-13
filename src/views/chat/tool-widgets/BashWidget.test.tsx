import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import BashWidget from "./BashWidget";
import type { BashDetail } from "@/lib/ipc";

describe("BashWidget (004-tool-call-widgets, US2)", () => {
  it("renders the command and stdout/stderr together, monospaced, distinguishable from prose (FR-003)", async () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "ls -la",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "a.txt\nb.txt", stderr: "" },
    };
    render(<BashWidget detail={detail} />);

    expect(screen.getByTestId("bash-command")).toHaveTextContent("ls -la");
    // Completed Bash widgets are collapsed by default; expand to see output.
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("bash-stdout")).toHaveTextContent("a.txt");
    expect(screen.getByTestId("bash-stdout")).toHaveTextContent("b.txt");
  });

  it("visually distinguishes success from failure via exitCode without requiring the output text to be read", async () => {
    const success: BashDetail = {
      toolName: "Bash",
      command: "true",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "", stderr: "" },
    };
    const { rerender } = render(<BashWidget detail={success} />);
    // Success is the quiet default: no badge at all on the collapsed row.
    expect(screen.getByTestId("bash-status").querySelector('[data-slot="badge"]')).toBeNull();

    const failure: BashDetail = {
      toolName: "Bash",
      command: "false",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 1, stdout: "", stderr: "boom" },
    };
    rerender(<BashWidget detail={failure} />);
    expect(screen.getByTestId("bash-status")).toHaveTextContent("Failed (exit 1)");
    await userEvent.click(screen.getByRole("button"));
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
    expect(screen.getByTestId("bash-status")).toHaveTextContent("Failed to run");
    expect(screen.getByTestId("bash-widget")).toHaveTextContent(/catastrophic command/);
    expect(
      screen.getByTestId("bash-widget").querySelector('[data-slot="badge"]'),
    ).toHaveTextContent("Failed");
  });

  it("truncates or collapses very long output rather than rendering it in full inline (FR-004)", async () => {
    const longOutput = Array.from({ length: 500 }, (_, i) => `line ${i}`).join("\n");
    const detail: BashDetail = {
      toolName: "Bash",
      command: "seq 1 500",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: longOutput, stderr: "" },
    };
    render(<BashWidget detail={detail} />);
    await userEvent.click(screen.getByRole("button"));

    const stdout = screen.getByTestId("bash-stdout");
    expect(stdout.textContent!.length).toBeLessThan(longOutput.length);
    expect(screen.getByTestId("bash-output-truncated")).toBeInTheDocument();
  });

  // --- 010-context-window-management/US3 ---

  it("shows a 'View full output' affordance when the result was offloaded", async () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "find / -name '*.log'",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "preview only...", stderr: "" },
      offloadedTo: "/data/tool-outputs/conv1/call1.txt",
    };
    render(<BashWidget detail={detail} />);
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
  });

  it("does not show the affordance when the result was not offloaded", async () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "echo hi",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "hi", stderr: "" },
      offloadedTo: null,
    };
    render(<BashWidget detail={detail} />);
    await userEvent.click(screen.getByRole("button"));
    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
  });

  it("shows exit code and token cost in the expanded panel footer, not the status row", async () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "cargo test --lib",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "ok", stderr: "" },
      tokenCount: 89,
    };
    render(<BashWidget detail={detail} />);
    expect(screen.getByTestId("bash-status")).not.toHaveTextContent("89 tok");
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("bash-meta")).toHaveTextContent("exit 0 · 89 tok");
  });

  // --- Task 9 (payload-files design): slimmed detail shapes ---

  it("renders the preview fields and offers the payload file", async () => {
    render(
      <BashWidget
        detail={{
          toolName: "Bash",
          command: "cargo test",
          timeoutMs: null,
          payloadRef: "/data/tool-outputs/c1/tc1.txt",
          outcome: {
            ok: true,
            exitCode: 0,
            stdoutPreview: "running 214 tests…",
            stdoutBytes: 48213,
            stderrPreview: "",
            stderrBytes: 0,
          },
        }}
      />,
    );
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("bash-stdout")).toHaveTextContent("running 214 tests…");
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
  });

  it("still renders legacy rows with inline stdout and offloadedTo", async () => {
    render(
      <BashWidget
        detail={{
          toolName: "Bash",
          command: "ls",
          timeoutMs: null,
          offloadedTo: "/old/offload.txt",
          outcome: { ok: true, exitCode: 0, stdout: "a.txt", stderr: "" },
        }}
      />,
    );
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("bash-stdout")).toHaveTextContent("a.txt");
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
  });

  // --- pending/running state (no outcome yet) ---

  it("renders a pending state (command shown, no outcome) as Running…", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "cargo test --test agent_benchmark tier4_planned",
      timeoutMs: null,
    };
    render(<BashWidget detail={detail} />);
    expect(screen.getByTestId("bash-status")).toHaveTextContent(/running/i);
    expect(screen.getByTestId("bash-widget").querySelector('[data-slot="spinner"]')).not.toBeNull();
    // Pending/running Bash stays expanded (defaultOpen) — command visible
    // without clicking, even though the header is still a collapsible trigger.
    expect(screen.getByTestId("bash-command")).toHaveTextContent(
      "cargo test --test agent_benchmark tier4_planned",
    );
  });

  // --- empty-output completed commands render header-only, not a
  // collapsible with an empty panel ---

  it("renders a header-only frame (no trigger, no content panel) when a completed command produced no output", () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "touch file.txt",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "", stderr: "" },
    };
    const { container } = render(<BashWidget detail={detail} />);

    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(container.querySelector('[data-slot="collapsible"]')).toBeNull();
    expect(screen.getByTestId("bash-command")).toHaveTextContent("touch file.txt");
    expect(screen.getByTestId("bash-status").querySelector('[data-slot="badge"]')).toBeNull();
  });

  it("collapses completed output by default until the header is clicked", async () => {
    const detail: BashDetail = {
      toolName: "Bash",
      command: "ls -la",
      timeoutMs: null,
      outcome: { ok: true, exitCode: 0, stdout: "a.txt", stderr: "" },
    };
    render(<BashWidget detail={detail} />);

    // Header (command, no badge) renders without expanding.
    expect(screen.getByTestId("bash-command")).toHaveTextContent("ls -la");
    expect(screen.getByTestId("bash-status").querySelector('[data-slot="badge"]')).toBeNull();
    expect(screen.queryByTestId("bash-stdout")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("bash-stdout")).toHaveTextContent("a.txt");
  });
});
