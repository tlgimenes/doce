import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
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
    const updatedRows = screen.getAllByTestId("plan-step");
    expect(updatedRows).toHaveLength(3);
    expect(updatedRows[2]).toHaveAttribute("data-current", "true");
    expect(updatedRows[1].querySelector('[data-slot="checkbox"]')).toHaveAttribute(
      "aria-checked",
      "true",
    );
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
    expect(screen.getByText("Fix bug_01.txt")).toHaveClass("line-through");

    // The stale recovery invoke resolves after the fresher event -- must
    // not clobber what the event already established.
    await act(async () => {
      resolveRecovery(snapshot());
    });
    expect(screen.getByText("Fix bug_01.txt")).toHaveClass("line-through");
  });

  it("renders every step in source order with Marker completion and current-state styling", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);

    const rows = await screen.findAllByTestId("plan-step");
    expect(rows).toHaveLength(3);
    expect(rows.map((row) => row.textContent)).toEqual([
      "Find all bug markers",
      "Fix bug_01.txt",
      "Fix bug_02.txt",
    ]);

    const checkboxes = rows.map((row) => row.querySelector('[data-slot="checkbox"]'));
    expect(checkboxes).toHaveLength(3);
    expect(checkboxes[0]).toHaveAttribute("aria-checked", "true");
    expect(checkboxes[1]).toHaveAttribute("aria-checked", "false");
    expect(checkboxes[2]).toHaveAttribute("aria-checked", "false");
    checkboxes.forEach((checkbox) =>
      expect(checkbox).toHaveAttribute("aria-disabled", "true"),
    );

    expect(screen.getByText("Find all bug markers")).toHaveClass("line-through");
    expect(rows[0]).toHaveClass("text-muted-foreground");
    expect(rows[1]).toHaveClass("text-foreground");
    expect(rows[2]).toHaveClass("text-muted-foreground");
    expect(rows[1]).toHaveAttribute("data-current", "true");
    expect(rows[0]).not.toHaveAttribute("data-current");
  });

  it("uses a three-row scroll viewport with Shadcn's bottom fade", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(
      snapshot({
        steps: [
          { description: "s0", done: true },
          { description: "s1", done: false },
          { description: "s2", done: false },
          { description: "s3", done: false },
        ],
        currentStepIndex: 1,
      }),
    );
    render(<PlanTracker conversationId="c1" />);

    const scroller = await screen.findByTestId("plan-task-scroller");
    expect(scroller).toHaveStyle({ maxHeight: "3.75rem" });
    expect(screen.getAllByTestId("plan-step")).toHaveLength(4);

    const viewport = screen.getByTestId("plan-task-viewport");
    expect(viewport).toHaveClass("overflow-y-auto", "scroll-fade-b");
  });
});
