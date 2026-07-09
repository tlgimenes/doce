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

    const card = await screen.findByTestId("plan-card");
    expect(card).toHaveTextContent("Fix the scattered bugs");
    expect(card).toHaveTextContent("1/3");
    const steps = screen.getAllByTestId("plan-step");
    expect(steps).toHaveLength(3);
    expect(steps[0]).toHaveClass("line-through");
    expect(steps[1]).toHaveAttribute("data-current", "true");

    // Check done step icon is green
    const doneCheckSpan = steps[0].querySelector("span.w-3");
    expect(doneCheckSpan).toHaveClass("text-emerald-600");

    // Check pending step has muted styling
    expect(steps[2]).toHaveClass("text-muted-foreground");
    // Check current step does not have muted styling
    expect(steps[1]).not.toHaveClass("text-muted-foreground");
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
    expect(screen.getByTestId("plan-card")).toHaveTextContent("2/3");
  });

  it("fades out and unmounts when the turn ends (plan: null)", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);
    await screen.findByTestId("plan-tracker");

    act(() => firePlanUpdate({ conversationId: "c1", plan: null }));
    // Fading: still mounted with the leaving style…
    expect(screen.getByTestId("plan-tracker")).toHaveClass("opacity-0");
    // …then gone.
    await waitFor(() => expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument());
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
    await screen.findByTestId("plan-card");

    expect(screen.getByTestId("plan-done-collapsed")).toHaveTextContent("3 done");
    // Current (s3) + up to 4 pending (s4..s7) visible, rest summarized.
    expect(screen.getAllByTestId("plan-step")).toHaveLength(5);
    expect(screen.getByTestId("plan-more")).toHaveTextContent("+1 more");
  });

  it("renders the dot rail (with matching states) alongside the card, and a chip past 12 steps", async () => {
    vi.mocked(commands.getActivePlan).mockResolvedValue(snapshot());
    render(<PlanTracker conversationId="c1" />);
    await screen.findByTestId("plan-rail");

    const dots = screen.getAllByTestId("plan-dot");
    expect(dots).toHaveLength(3);
    expect(dots[0]).toHaveTextContent("✓");
    expect(dots[1]).toHaveAttribute("data-current", "true");
    expect(screen.queryByTestId("plan-chip")).not.toBeInTheDocument();

    act(() =>
      firePlanUpdate({
        conversationId: "c1",
        plan: snapshot({
          steps: Array.from({ length: 13 }, (_, i) => ({
            description: `s${i}`,
            done: i < 5,
          })),
          currentStepIndex: 5,
        }),
      }),
    );
    expect(screen.queryAllByTestId("plan-dot")).toHaveLength(0);
    expect(screen.getByTestId("plan-chip")).toHaveTextContent("5/13");
  });
});
