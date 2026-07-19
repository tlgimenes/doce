import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
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

  it("uses the logo-forward onboarding shell", async () => {
    vi.mocked(commands.getHardwareProfile).mockResolvedValue({
      tier: "apple-silicon-16gb",
      ramGb: 16,
      chip: "Apple M2",
      diskFreeGb: 200,
    });

    render(<Onboarding onReady={() => {}} />);

    expect(await screen.findByAltText("doce")).toHaveClass("h-24");
    expect(screen.getByText("doce")).toBeInTheDocument();
  });

  it("waits for a healthy active model instead of entering after download alone", async () => {
    vi.mocked(commands.getHardwareProfile).mockResolvedValue({
      tier: "apple-silicon-16gb",
      ramGb: 16,
      chip: "Apple M2",
      diskFreeGb: 200,
    });
    let progress:
      | ((payload: {
          modelId: string;
          bytesDownloaded: number;
          bytesTotal: number;
          state: string;
        }) => void)
      | null = null;
    vi.mocked(events.onModelInstallProgress).mockImplementation(async (callback) => {
      progress = callback;
      return () => {};
    });
    const onReady = vi.fn();
    render(<Onboarding onReady={onReady} />);
    await waitFor(() => expect(progress).not.toBeNull());

    act(() =>
      progress?.({ modelId: "m", bytesDownloaded: 10, bytesTotal: 10, state: "downloaded" }),
    );
    expect(onReady).not.toHaveBeenCalled();
    expect(screen.getByText("Getting the model ready…")).toBeInTheDocument();

    act(() => progress?.({ modelId: "m", bytesDownloaded: 0, bytesTotal: 0, state: "active" }));
    expect(onReady).toHaveBeenCalledTimes(1);
  });
});
