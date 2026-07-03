import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import Onboarding from "./Onboarding";
import { commands, events } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    getHardwareProfile: vi.fn(),
    startModelInstall: vi.fn(),
  },
  events: {
    onModelInstallProgress: vi.fn(),
  },
}));

describe("Onboarding", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(events.onModelInstallProgress).mockResolvedValue(() => {});
    vi.mocked(commands.startModelInstall).mockResolvedValue({
      modelId: "test-model",
      resumed: false,
    });
  });

  it("shows no model picker, API key field, or account step (FR-001)", async () => {
    vi.mocked(commands.getHardwareProfile).mockResolvedValue({
      tier: "apple-silicon-16gb",
      ramGb: 16,
      chip: "Apple M2",
      diskFreeGb: 200,
    });

    render(<Onboarding onReady={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText(/Apple M2/)).toBeInTheDocument();
    });

    expect(screen.queryByRole("textbox", { name: /api key/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(commands.startModelInstall).toHaveBeenCalledWith();
  });

  it("surfaces a hardware-detection failure instead of hanging silently", async () => {
    vi.mocked(commands.getHardwareProfile).mockRejectedValue(new Error("sysctl failed"));

    render(<Onboarding onReady={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText(/sysctl failed/)).toBeInTheDocument();
    });
  });
});
