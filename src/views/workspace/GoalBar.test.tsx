import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import GoalBar from "./GoalBar";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc")>();
  return {
    ...actual,
    commands: {
      getConversationGoal: vi.fn(),
      setConversationGoal: vi.fn(),
    },
  };
});

describe("GoalBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the 'Set a goal' affordance when no goal is set", async () => {
    vi.mocked(commands.getConversationGoal).mockResolvedValue(null);

    render(<GoalBar conversationId="conv-1" />);

    await waitFor(() => expect(commands.getConversationGoal).toHaveBeenCalledWith("conv-1"));
    expect(await screen.findByTestId("goal-bar-set-affordance")).toHaveTextContent("Set a goal");
    expect(screen.queryByTestId("goal-bar-display")).not.toBeInTheDocument();
  });

  it("renders the current goal loaded from getConversationGoal", async () => {
    vi.mocked(commands.getConversationGoal).mockResolvedValue("Ship the login page");

    render(<GoalBar conversationId="conv-1" />);

    expect(await screen.findByTestId("goal-bar-display")).toHaveTextContent("Ship the login page");
    expect(screen.queryByTestId("goal-bar-set-affordance")).not.toBeInTheDocument();
  });

  it("sets a goal: typing into the editor and saving calls setConversationGoal with the typed value", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.getConversationGoal).mockResolvedValue(null);
    vi.mocked(commands.setConversationGoal).mockResolvedValue(undefined);

    render(<GoalBar conversationId="conv-1" />);

    await user.click(await screen.findByTestId("goal-bar-set-affordance"));
    const input = await screen.findByTestId("goal-bar-input");
    await user.type(input, "Ship the login page");
    await user.click(screen.getByTestId("goal-bar-save"));

    await waitFor(() =>
      expect(commands.setConversationGoal).toHaveBeenCalledWith("conv-1", "Ship the login page"),
    );
    expect(await screen.findByTestId("goal-bar-display")).toHaveTextContent("Ship the login page");
  });

  it("clears a goal: saving an empty (whitespace-only) draft calls setConversationGoal with null", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.getConversationGoal).mockResolvedValue("Ship the login page");
    vi.mocked(commands.setConversationGoal).mockResolvedValue(undefined);

    render(<GoalBar conversationId="conv-1" />);

    await user.click(await screen.findByTestId("goal-bar-display"));
    const input = await screen.findByTestId("goal-bar-input");
    await user.clear(input);
    await user.type(input, "   ");
    await user.click(screen.getByTestId("goal-bar-save"));

    await waitFor(() => expect(commands.setConversationGoal).toHaveBeenCalledWith("conv-1", null));
    expect(await screen.findByTestId("goal-bar-set-affordance")).toBeInTheDocument();
  });

  it("cancelling an edit leaves the previously saved goal untouched", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.getConversationGoal).mockResolvedValue("Ship the login page");

    render(<GoalBar conversationId="conv-1" />);

    await user.click(await screen.findByTestId("goal-bar-display"));
    const input = await screen.findByTestId("goal-bar-input");
    await user.clear(input);
    await user.type(input, "something else entirely");
    await user.click(screen.getByTestId("goal-bar-cancel"));

    expect(commands.setConversationGoal).not.toHaveBeenCalled();
    expect(await screen.findByTestId("goal-bar-display")).toHaveTextContent("Ship the login page");
  });

  it("a failed getConversationGoal degrades to the unset-goal affordance instead of crashing", async () => {
    vi.mocked(commands.getConversationGoal).mockRejectedValue(new Error("no db"));

    render(<GoalBar conversationId="conv-1" />);

    expect(await screen.findByTestId("goal-bar-set-affordance")).toBeInTheDocument();
  });
});
