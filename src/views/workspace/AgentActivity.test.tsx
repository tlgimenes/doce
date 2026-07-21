import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PlanSnapshot } from "@/lib/ipc";
import { AgentActivityView, currentThinkingLine, formatElapsedMs } from "./AgentActivity";

const noopGoal = (overrides: Partial<Parameters<typeof AgentActivityView>[0]["goal"]> = {}) => ({
  current: null,
  achieved: false,
  onEdit: vi.fn(),
  onDelete: vi.fn(),
  ...overrides,
});

const idleWorking = { active: false, elapsedLabel: null, tokens: null, thinkingLine: null };

const plan = (overrides: Partial<PlanSnapshot> = {}): PlanSnapshot => ({
  goal: "",
  currentStepIndex: 1,
  steps: [
    { description: "Read the auth module", done: true },
    { description: "Add the login form fields", done: false },
    { description: "Wire the submit handler", done: false },
  ],
  ...overrides,
});

describe("currentThinkingLine", () => {
  it("returns the latest non-empty think line", () => {
    expect(currentThinkingLine("<think>\nfirst thought\nsecond thought")).toBe("second thought");
  });

  it("returns null once thinking closes or a tool call begins", () => {
    expect(currentThinkingLine("<think>\nsettled\n</think>")).toBeNull();
    expect(currentThinkingLine('<think>\nplan\n</think><tool_call>{"name":"Read"')).toBeNull();
    expect(currentThinkingLine('<function name="Read"')).toBeNull();
  });

  it("suppresses a partially-sampled marker instead of flickering it", () => {
    expect(currentThinkingLine("<think>\nreal line\n<fun")).toBe("real line");
  });

  it("shows even a degenerate line verbatim — a window, not a censor", () => {
    expect(currentThinkingLine("<think>\naaaaaaaa")).toBe("aaaaaaaa");
  });
});

describe("formatElapsedMs", () => {
  it("formats seconds to one decimal and floors negatives at zero", () => {
    expect(formatElapsedMs(12340)).toBe("12.3s");
    expect(formatElapsedMs(-50)).toBe("0.0s");
  });
});

describe("AgentActivityView", () => {
  it("renders nothing when there is no goal, no plan, and no live turn", () => {
    const { container } = render(
      <AgentActivityView plan={null} goal={noopGoal()} working={idleWorking} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the goal in the growing primary slot", () => {
    render(
      <AgentActivityView
        plan={null}
        goal={noopGoal({ current: "Ship the login flow" })}
        working={idleWorking}
      />,
    );
    expect(screen.getByTestId("agent-activity-goal")).toHaveTextContent("Ship the login flow");
    expect(screen.queryByTestId("agent-activity-current-todo")).not.toBeInTheDocument();
  });

  it("falls back to the current todo in the primary slot when there is no goal", () => {
    render(<AgentActivityView plan={plan()} goal={noopGoal()} working={idleWorking} />);
    expect(screen.queryByTestId("agent-activity-goal")).not.toBeInTheDocument();
    expect(screen.getByTestId("agent-activity-current-todo")).toHaveTextContent(
      "Add the login form fields",
    );
  });

  it("renders progress with a done/total count when a plan is present", () => {
    render(<AgentActivityView plan={plan()} goal={noopGoal()} working={idleWorking} />);
    expect(screen.getByTestId("plan-status")).toHaveTextContent("1/3");
    expect(screen.getByTestId("plan-tracker")).toBeInTheDocument();
  });

  it("renders the working indicator with an accessible label, chron, and token totals", () => {
    render(
      <AgentActivityView
        plan={null}
        goal={noopGoal()}
        working={{
          active: true,
          elapsedLabel: "4.2s",
          tokens: { input: 1042, output: 320 },
          thinkingLine: null,
        }}
      />,
    );
    expect(screen.getByTestId("agent-thinking")).toBeInTheDocument();
    expect(screen.getByTestId("agent-thinking-status")).toHaveTextContent("Working");
    expect(screen.getByTestId("agent-thinking-timer")).toHaveTextContent("4.2s");
    expect(screen.getByTestId("agent-thinking-tokens")).toHaveTextContent("↑ 1.0k ↓ 320");
  });

  it("hides a zero-valued token direction", () => {
    render(
      <AgentActivityView
        plan={null}
        goal={noopGoal()}
        working={{
          active: true,
          elapsedLabel: "1.0s",
          tokens: { input: 5, output: 0 },
          thinkingLine: null,
        }}
      />,
    );
    const tokens = screen.getByTestId("agent-thinking-tokens");
    expect(tokens).toHaveTextContent("↑ 5");
    expect(tokens).not.toHaveTextContent("↓");
  });

  it("fills the primary slot with the reasoning line when there is no goal or todo, and hides it when not reasoning", () => {
    const { rerender } = render(
      <AgentActivityView
        plan={null}
        goal={noopGoal()}
        working={{
          active: true,
          elapsedLabel: "1.0s",
          tokens: null,
          thinkingLine: "checking the session shape",
        }}
      />,
    );
    expect(screen.getByTestId("agent-thinking-stream")).toHaveTextContent(
      "checking the session shape",
    );

    rerender(
      <AgentActivityView
        plan={null}
        goal={noopGoal()}
        working={{ active: true, elapsedLabel: "1.0s", tokens: null, thinkingLine: null }}
      />,
    );
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();
  });

  it("shows a static 'Thinking…' in the primary slot while working with no goal, todo, or reasoning line", () => {
    render(
      <AgentActivityView
        plan={null}
        goal={noopGoal()}
        working={{ active: true, elapsedLabel: "0.4s", tokens: null, thinkingLine: null }}
      />,
    );
    expect(screen.getByTestId("agent-thinking-fallback")).toHaveTextContent("Thinking");
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();
  });

  it("suppresses the reasoning line when a goal holds the primary slot (goal wins)", () => {
    render(
      <AgentActivityView
        plan={null}
        goal={noopGoal({ current: "Ship the login flow" })}
        working={{
          active: true,
          elapsedLabel: "1.0s",
          tokens: null,
          thinkingLine: "checking the session shape",
        }}
      />,
    );
    expect(screen.getByTestId("agent-activity-goal")).toHaveTextContent("Ship the login flow");
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();
  });

  it("suppresses the reasoning line when a todo holds the primary slot (todo wins)", () => {
    render(
      <AgentActivityView
        plan={plan()}
        goal={noopGoal()}
        working={{
          active: true,
          elapsedLabel: "1.0s",
          tokens: null,
          thinkingLine: "checking the session shape",
        }}
      />,
    );
    expect(screen.getByTestId("agent-activity-current-todo")).toBeInTheDocument();
    expect(screen.queryByTestId("agent-thinking-stream")).not.toBeInTheDocument();
  });

  it("has no expander when there is only a live turn (nothing to reveal)", () => {
    render(
      <AgentActivityView
        plan={null}
        goal={noopGoal()}
        working={{ active: true, elapsedLabel: "2.0s", tokens: null, thinkingLine: null }}
      />,
    );
    expect(screen.queryByTestId("agent-activity-expander")).not.toBeInTheDocument();
  });

  it("expands when clicking anywhere on the strip (not just the chevron), and marks it as an expandable button", async () => {
    render(<AgentActivityView plan={plan()} goal={noopGoal()} working={idleWorking} />);
    const strip = screen.getByTestId("plan-tracker");
    expect(strip).toHaveAttribute("role", "button");
    expect(strip).toHaveClass("cursor-pointer");
    expect(screen.queryAllByTestId("plan-step")).toHaveLength(0);

    // Click the strip body (the current-todo text), not the chevron.
    await userEvent.click(screen.getByTestId("agent-activity-current-todo"));
    expect(screen.getAllByTestId("plan-step")).toHaveLength(3);
  });

  it("expands to reveal the full plan checklist in source order", async () => {
    render(<AgentActivityView plan={plan()} goal={noopGoal()} working={idleWorking} />);
    expect(screen.queryAllByTestId("plan-step")).toHaveLength(0);

    await userEvent.click(screen.getByTestId("agent-activity-expander"));

    const steps = screen.getAllByTestId("plan-step");
    expect(steps).toHaveLength(3);
    expect(steps[0]).toHaveTextContent("Read the auth module");
    expect(steps[0]).toHaveAttribute("data-state", "done");
    expect(steps[1]).toHaveAttribute("data-current", "true");
  });

  it("expands to reveal goal edit/delete controls that fire their callbacks", async () => {
    const goal = noopGoal({ current: "Ship the login flow" });
    render(<AgentActivityView plan={null} goal={goal} working={idleWorking} />);

    await userEvent.click(screen.getByTestId("agent-activity-expander"));
    await userEvent.click(screen.getByTestId("agent-activity-goal-edit"));
    expect(goal.onEdit).toHaveBeenCalledTimes(1);

    await userEvent.click(screen.getByTestId("agent-activity-goal-delete"));
    expect(goal.onDelete).toHaveBeenCalledTimes(1);
  });

  it("renders an achieved goal muted with no edit/delete controls", async () => {
    const goal = noopGoal({ current: "Ship the login flow", achieved: true });
    render(<AgentActivityView plan={plan()} goal={goal} working={idleWorking} />);

    expect(screen.getByTestId("agent-activity-goal")).toHaveClass("text-muted-foreground");
    // Expanding still shows the plan, but never the goal controls for a done goal.
    await userEvent.click(screen.getByTestId("agent-activity-expander"));
    expect(screen.queryByTestId("agent-activity-goal-edit")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agent-activity-goal-delete")).not.toBeInTheDocument();
  });
});
