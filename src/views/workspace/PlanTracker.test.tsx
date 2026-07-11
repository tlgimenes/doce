import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import PlanTracker from "./PlanTracker";
import { commands, events } from "@/lib/ipc";
import type { PlanSnapshot, PlanUpdatePayload } from "@/lib/ipc";

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc")>();
  return {
    ...actual,
    commands: { getActivePlan: vi.fn() },
    events: { onPlanUpdate: vi.fn() },
  };
});

function snapshot(overrides: Partial<PlanSnapshot> = {}): PlanSnapshot {
  return {
    goal: "Fix the scattered bugs",
    steps: [
      { description: "Find all bug markers", done: true },
      { description: "Fix bug_01.txt", done: false },
      { description: "Fix bug_02.txt", done: false },
    ],
    currentStepIndex: 1,
    ...overrides,
  };
}

describe("PlanTracker", () => {
  let firePlanUpdate: (p: PlanUpdatePayload) => void;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.getActivePlan).mockResolvedValue(null);
    vi.mocked(events.onPlanUpdate).mockImplementation(async (cb) => {
      firePlanUpdate = cb;
      return () => {};
    });
  });

  it("renders nothing when no plan is active", async () => {
    const { container } = render(<PlanTracker conversationId="c1" />);
    await waitFor(() => expect(commands.getActivePlan).toHaveBeenCalledWith("c1"));
    expect(container).toBeEmptyDOMElement();
  });

  it("recovers an in-flight plan on mount (reload case)", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);

    expect(await screen.findByTestId("plan-tracker")).toBeInTheDocument();
  });

  it("appears and updates on plan-update events for its own conversation only", async () => {
    render(<PlanTracker conversationId="c1" />);
    await waitFor(() => expect(events.onPlanUpdate).toHaveBeenCalled());

    act(() => firePlanUpdate({ conversationId: "other", plan: snapshot() }));
    expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument();

    act(() => firePlanUpdate({ conversationId: "c1", plan: snapshot() }));
    expect(await screen.findByTestId("plan-tracker")).toBeInTheDocument();

    act(() =>
      firePlanUpdate({
        conversationId: "c1",
        plan: snapshot({
          steps: [
            { description: "Find all bug markers", done: true },
            { description: "Fix bug_01.txt", done: true },
            { description: "Fix bug_02.txt", done: false },
          ],
          currentStepIndex: 2,
        }),
      }),
    );
    expect(screen.getByTestId("plan-current-step")).toHaveTextContent("2/3");
  });

  it("unmounts when the turn ends (plan: null)", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);
    await screen.findByTestId("plan-tracker");

    act(() => firePlanUpdate({ conversationId: "c1", plan: null }));
    await waitFor(() => expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument());
  });

  it("ignores a late-resolving recovery snapshot once a plan-update null event already ended the turn (stuck-tracker race)", async () => {
    let resolveRecovery!: (v: PlanSnapshot | null) => void;
    vi.mocked(commands.getActivePlan).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRecovery = resolve;
        }),
    );
    render(<PlanTracker conversationId="c1" />);
    await waitFor(() => expect(events.onPlanUpdate).toHaveBeenCalled());

    // The turn ends (plan: null) before the recovery invoke has resolved.
    act(() => firePlanUpdate({ conversationId: "c1", plan: null }));
    expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument();

    // The recovery invoke resolves late with a stale snapshot -- it must
    // not resurrect the tracker for a turn that has already ended.
    await act(async () => {
      resolveRecovery(snapshot());
    });
    expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument();
  });

  it("keeps the fresher plan-update snapshot when a stale recovery invoke resolves after it (stale-clobber race)", async () => {
    let resolveRecovery!: (v: PlanSnapshot | null) => void;
    vi.mocked(commands.getActivePlan).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRecovery = resolve;
        }),
    );
    render(<PlanTracker conversationId="c1" />);
    await waitFor(() => expect(events.onPlanUpdate).toHaveBeenCalled());

    const fresherPlan = snapshot({
      steps: [
        { description: "Find all bug markers", done: true },
        { description: "Fix bug_01.txt", done: true },
        { description: "Fix bug_02.txt", done: false },
      ],
      currentStepIndex: 2,
    });
    act(() => firePlanUpdate({ conversationId: "c1", plan: fresherPlan }));
    expect(screen.getByTestId("plan-current-step")).toHaveTextContent("2/3");

    // The stale recovery invoke resolves after the fresher event -- must
    // not clobber what the event already established.
    await act(async () => {
      resolveRecovery(snapshot());
    });
    expect(screen.getByTestId("plan-current-step")).toHaveTextContent("2/3");
  });

  it("collapses completed steps and caps pending once the plan exceeds 6 steps", async () => {
    const many = snapshot({
      steps: [
        { description: "s0", done: true },
        { description: "s1", done: true },
        { description: "s2", done: true },
        { description: "s3", done: false },
        { description: "s4", done: false },
        { description: "s5", done: false },
        { description: "s6", done: false },
        { description: "s7", done: false },
        { description: "s8", done: false },
      ],
      currentStepIndex: 3,
    });
    vi.mocked(commands.getActivePlan).mockResolvedValue(many);
    render(<PlanTracker conversationId="c1" />);
    await userEvent.click(await screen.findByTestId("plan-current-step"));

    expect(screen.getByTestId("plan-done-collapsed")).toHaveTextContent("3 done");
    // Current (s3) + up to 4 pending (s4..s7) visible, rest summarized.
    expect(screen.getAllByTestId("plan-step")).toHaveLength(5);
    expect(screen.getByTestId("plan-more")).toHaveTextContent("+1 more");
  });

  it("shows the current step and progress in the collapsed one-liner", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue({
      goal: "Ship the feature",
      currentStepIndex: 1,
      steps: [
        { description: "Write tests", done: true },
        { description: "Implement", done: false },
        { description: "Verify", done: false },
      ],
    });
    render(<PlanTracker conversationId="conv-1" />);

    const trigger = await screen.findByTestId("plan-current-step");
    expect(trigger).toHaveTextContent("Implement");
    expect(trigger).toHaveTextContent("1/3");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    // Spinner removed by user directive — no spinner anywhere in the panel.
    expect(trigger.querySelector('[data-slot="spinner"]')).toBeNull();
  });

  it("falls back to the goal while planning (currentStepIndex null)", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue({
      goal: "Ship the feature",
      currentStepIndex: null,
      steps: [{ description: "Write tests", done: false }],
    });
    render(<PlanTracker conversationId="conv-1" />);

    expect(await screen.findByTestId("plan-current-step")).toHaveTextContent("Ship the feature");
  });

  it("expands upward into the full step list", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue({
      goal: "Ship the feature",
      currentStepIndex: 1,
      steps: [
        { description: "Write tests", done: true },
        { description: "Implement", done: false },
        { description: "Verify", done: false },
      ],
    });
    render(<PlanTracker conversationId="conv-1" />);
    await userEvent.click(await screen.findByTestId("plan-current-step"));

    const steps = screen.getAllByTestId("plan-step");
    expect(steps).toHaveLength(3);
    expect(steps[0]).toHaveAttribute("data-state", "done");
    expect(steps[1]).toHaveAttribute("data-state", "current");
    // The list renders BEFORE the trigger in DOM order (upward expansion).
    expect(
      steps[0].compareDocumentPosition(screen.getByTestId("plan-current-step")) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("shows a Check icon in the one-liner when every step is done", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue({
      goal: "Ship the feature",
      currentStepIndex: null,
      steps: [{ description: "Write tests", done: true }],
    });
    render(<PlanTracker conversationId="conv-1" />);

    const trigger = await screen.findByTestId("plan-current-step");
    expect(trigger.querySelector('[data-slot="spinner"]')).toBeNull();
    expect(trigger).toHaveTextContent("1/1");
  });
});
