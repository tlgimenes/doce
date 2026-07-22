import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, type ModelInstallProgressPayload } from "@/lib/ipc";
import ModelSelector from "./ModelSelector";

const progressEvents = vi.hoisted(() => ({
  callback: null as ((payload: ModelInstallProgressPayload) => void) | null,
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

vi.mock("@/lib/ipc", () => ({
  commands: {
    getModelState: vi.fn(),
    selectCuratedModel: vi.fn(),
    selectLocalModel: vi.fn(),
    selectEndpointModel: vi.fn(),
    testModelEndpoint: vi.fn(),
    pauseModelDownload: vi.fn(),
    resumeModelDownload: vi.fn(),
    stopModelDownload: vi.fn(),
    dismissModelNotice: vi.fn(),
  },
  events: {
    onModelInstallProgress: vi.fn(async (callback) => {
      progressEvents.callback = callback;
      return progressEvents.unlisten;
    }),
  },
}));

type State = Awaited<ReturnType<typeof commands.getModelState>>;
type Option = State["options"][number];
type Download = State["downloads"][number];

function modelOption(overrides: Partial<Option> = {}): Option {
  return {
    id: "balanced",
    displayName: "Balanced",
    description: "Fast and efficient for everyday work.",
    technicalName: "Qwen 3.5 4B",
    parameterCount: "4B",
    quantization: "Q4_K_M",
    sizeBytes: 2_700_000_000,
    recommended: true,
    installed: true,
    active: true,
    selected: true,
    sourceKind: "curated",
    localPath: null,
    endpointUrl: null,
    endpointModel: null,
    state: "active",
    bytesDownloaded: 2_700_000_000,
    bytesTotal: 2_700_000_000,
    ...overrides,
  } as Option;
}

function modelState(overrides: Partial<State> = {}): State {
  return {
    hardware: { tier: "32gb", ramGb: 32, chip: "Apple M3", diskFreeGb: 120 },
    options: [modelOption()],
    activeId: "balanced",
    selectedId: "balanced",
    fallbackNotice: null,
    downloads: [],
    ...overrides,
  } as State;
}

function modelDownload(overrides: Partial<Download> = {}): Download {
  return {
    modelId: "capable",
    displayName: "More capable",
    state: "downloading",
    bytesDownloaded: 1_000,
    bytesTotal: 2_000,
    revision: 1,
    error: null,
    ...overrides,
  } as Download;
}

function progressEvent(
  overrides: Partial<ModelInstallProgressPayload> = {},
): ModelInstallProgressPayload {
  return {
    modelId: "capable",
    state: "downloading",
    bytesDownloaded: 1_000,
    bytesTotal: 2_000,
    revision: 1,
    error: null,
    ...overrides,
  };
}

const capable = (overrides: Partial<Option> = {}) =>
  modelOption({
    id: "capable",
    displayName: "More capable",
    description: "More room for complex work.",
    technicalName: "Qwen 3.5 8B",
    parameterCount: "8B",
    sizeBytes: 5_100_000_000,
    recommended: false,
    installed: false,
    active: false,
    selected: false,
    state: "available",
    bytesDownloaded: 0,
    bytesTotal: 5_100_000_000,
    ...overrides,
  });

describe("ModelSelector", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    progressEvents.callback = null;
    vi.mocked(open).mockResolvedValue(null);
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({ options: [modelOption(), capable()] }),
    );
  });

  it("keeps useful model context without the original technical clutter", async () => {
    render(<ModelSelector />);

    const trigger = await screen.findByTestId("model-selector-trigger");
    expect(trigger).toHaveTextContent("Balanced");
    expect(trigger).not.toHaveTextContent("Qwen 3.5 4B");
    expect(screen.getByText("AI model")).toBeVisible();
    expect(
      screen.getByText("Used for every conversation and task. Doce downloads it when needed."),
    ).toBeVisible();
    expect(screen.getByTestId("choose-local-model-button")).toHaveTextContent("Choose…");
    expect(screen.getByText("Model from this Mac")).toBeVisible();
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Active");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent(
      "Fast and efficient for everyday work.",
    );
    expect(screen.queryByTestId("model-recommendation")).not.toBeInTheDocument();
    expect(screen.queryByTestId("model-technical-details")).not.toBeInTheDocument();

    await userEvent.click(trigger);
    expect(await screen.findByTestId("model-option-balanced")).toHaveTextContent("Balanced");
    expect(screen.getAllByText("Recommended")).toHaveLength(1);
    expect(screen.getAllByText("Fast and efficient for everyday work.")).toHaveLength(2);
    expect(screen.getByTestId("model-option-balanced")).toHaveTextContent("Ready to use");
    expect(screen.getByTestId("model-option-capable")).toHaveTextContent("Downloads when selected");
    expect(screen.queryByText("Qwen 3.5 4B")).not.toBeInTheDocument();
  });

  it("selects a curated model and keeps the old model active during download", async () => {
    const initial = modelState({ options: [modelOption(), capable()] });
    const pending = modelState({
      options: [
        modelOption({ selected: false }),
        capable({
          selected: true,
          state: "downloading",
          bytesDownloaded: 1_000,
          bytesTotal: 2_000,
        }),
      ],
      selectedId: "capable",
      downloads: [modelDownload()],
    });
    vi.mocked(commands.getModelState).mockResolvedValueOnce(initial).mockResolvedValue(pending);
    vi.mocked(commands.selectCuratedModel).mockResolvedValue(pending);

    render(<ModelSelector />);
    await userEvent.click(await screen.findByTestId("model-selector-trigger"));
    await userEvent.click(await screen.findByTestId("model-option-capable"));

    await waitFor(() => expect(commands.selectCuratedModel).toHaveBeenCalledWith("capable"));
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Active");
    expect(await screen.findByTestId("model-download-capable")).toHaveTextContent("50%");
    expect(screen.getByRole("progressbar", { name: "Downloading More capable" })).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
    expect(screen.getByText(/Balanced stays active while this finishes/)).toBeVisible();

    await waitFor(() => expect(progressEvents.callback).not.toBeNull());
    act(() => {
      progressEvents.callback?.(
        progressEvent({ bytesDownloaded: 3_000, bytesTotal: 4_000, revision: 2 }),
      );
    });
    expect(screen.getByTestId("model-download-capable")).toHaveTextContent("75%");
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "75");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
  });

  it("commits an active progress event even when the follow-up snapshot fails", async () => {
    vi.mocked(commands.getModelState)
      .mockResolvedValueOnce(
        modelState({
          options: [
            modelOption({ selected: false }),
            capable({ selected: true, state: "preparing" }),
          ],
          selectedId: "capable",
          downloads: [modelDownload({ state: "verifying" })],
        }),
      )
      .mockRejectedValueOnce(new Error("temporary read failure"));

    render(<ModelSelector />);
    await screen.findByTestId("model-selector-trigger");
    await waitFor(() => expect(progressEvents.callback).not.toBeNull());

    act(() => {
      progressEvents.callback?.(
        progressEvent({
          bytesDownloaded: 5_100_000_000,
          bytesTotal: 5_100_000_000,
          state: "active",
          revision: 2,
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("active-model-summary")).toHaveTextContent("More capable");
      expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Active");
    });
    expect(screen.queryByTestId("model-download-capable")).not.toBeInTheDocument();
  });

  it("shows an installed model choice immediately while the old model remains active", async () => {
    let resolveSelection: ((state: State) => void) | undefined;
    const installedCapable = capable({ installed: true, state: "ready" });
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({ options: [modelOption(), installedCapable] }),
    );
    vi.mocked(commands.selectCuratedModel).mockImplementation(
      () =>
        new Promise<State>((resolve) => {
          resolveSelection = resolve;
        }),
    );

    render(<ModelSelector />);
    const trigger = await screen.findByTestId("model-selector-trigger");
    await userEvent.click(trigger);
    await userEvent.click(await screen.findByTestId("model-option-capable"));

    await waitFor(() => expect(commands.selectCuratedModel).toHaveBeenCalledWith("capable"));
    expect(trigger).toHaveTextContent("More capable");
    expect(trigger.querySelector(".lucide-chevrons-up-down")).toBeInTheDocument();
    expect(trigger.querySelector('[data-slot="spinner"]')).not.toBeInTheDocument();
    expect(trigger).not.toHaveAttribute("aria-busy", "true");

    const activeSummary = screen.getByTestId("active-model-summary");
    expect(activeSummary).toHaveTextContent("Balanced");
    expect(activeSummary).toHaveTextContent("Active");
    expect(activeSummary).toHaveTextContent("Switching to More capable…");
    expect(within(activeSummary).getByRole("status", { name: "Loading" })).toBeVisible();

    await act(async () => {
      resolveSelection?.(
        modelState({
          options: [
            modelOption({ active: false, selected: false, state: "ready" }),
            capable({
              installed: true,
              active: true,
              selected: true,
              state: "active",
            }),
          ],
          activeId: "capable",
          selectedId: "capable",
        }),
      );
    });

    await waitFor(() => expect(activeSummary).toHaveTextContent("More capable"));
    expect(activeSummary).toHaveTextContent("Active");
    expect(activeSummary).toHaveTextContent("More room for complex work.");
    expect(activeSummary).not.toHaveTextContent("Switching to");
    expect(
      within(activeSummary).queryByRole("status", { name: "Loading" }),
    ).not.toBeInTheDocument();
    expect(trigger.querySelector(".lucide-chevrons-up-down")).toBeInTheDocument();
  });

  it("does not let an older active event clear a newer requested model", async () => {
    const focused = modelOption({
      id: "focused",
      displayName: "Focused",
      description: "Designed for focused professional work.",
      technicalName: "Focused 4B",
      recommended: false,
      installed: true,
      active: false,
      selected: false,
      state: "ready",
    });
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ installed: true, state: "ready" }), focused],
      }),
    );
    vi.mocked(commands.selectCuratedModel).mockReturnValue(new Promise(() => undefined));

    render(<ModelSelector />);
    const trigger = await screen.findByTestId("model-selector-trigger");
    await waitFor(() => expect(progressEvents.callback).not.toBeNull());

    await userEvent.click(trigger);
    await userEvent.click(await screen.findByTestId("model-option-capable"));
    expect(trigger).toHaveTextContent("More capable");

    await userEvent.click(trigger);
    await userEvent.click(await screen.findByTestId("model-option-focused"));
    expect(trigger).toHaveTextContent("Focused");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Switching to Focused…");

    act(() => {
      progressEvents.callback?.(
        progressEvent({
          modelId: "capable",
          state: "active",
          bytesDownloaded: 5_100_000_000,
          bytesTotal: 5_100_000_000,
          revision: 8,
        }),
      );
    });

    expect(trigger).toHaveTextContent("Focused");
    expect(trigger.querySelector(".lucide-chevrons-up-down")).toBeInTheDocument();
    const activeSummary = screen.getByTestId("active-model-summary");
    expect(activeSummary).toHaveTextContent("More capable");
    expect(activeSummary).toHaveTextContent("Active");
    expect(activeSummary).toHaveTextContent("Switching to Focused…");
    expect(activeSummary).not.toHaveTextContent("Switching to More capable");
  });

  it("lets a newer choice supersede an installed-model handoff that is still waiting", async () => {
    let resolveFirst: ((state: State) => void) | undefined;
    const firstRequest = new Promise<State>((resolve) => {
      resolveFirst = resolve;
    });
    vi.mocked(commands.selectCuratedModel).mockImplementation((modelId) =>
      modelId === "capable"
        ? firstRequest
        : Promise.resolve(modelState({ options: [modelOption(), capable()] })),
    );

    render(<ModelSelector />);
    const trigger = await screen.findByTestId("model-selector-trigger");
    await userEvent.click(trigger);
    await userEvent.click(await screen.findByTestId("model-option-capable"));

    expect(trigger).not.toBeDisabled();
    await userEvent.click(trigger);
    await userEvent.click(await screen.findByTestId("model-option-balanced"));
    await waitFor(() => expect(commands.selectCuratedModel).toHaveBeenLastCalledWith("balanced"));
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");

    resolveFirst?.(
      modelState({
        options: [modelOption({ selected: false }), capable({ selected: true })],
        selectedId: "capable",
      }),
    );
    await act(async () => Promise.resolve());
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
    expect(screen.getByTestId("model-selector-trigger")).toHaveTextContent("Balanced");
    expect(screen.queryByTestId("model-progress")).not.toBeInTheDocument();
  });

  it("lets a curated choice supersede an unresolved local-model handoff", async () => {
    let resolveLocal: ((state: State) => void) | undefined;
    vi.mocked(open).mockResolvedValue("/Users/maya/Models/atlas-4b.gguf");
    vi.mocked(commands.selectLocalModel).mockImplementation(
      () =>
        new Promise<State>((resolve) => {
          resolveLocal = resolve;
        }),
    );
    const installedCapable = capable({ installed: true, state: "installed" });
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({ options: [modelOption(), installedCapable] }),
    );
    vi.mocked(commands.selectCuratedModel).mockResolvedValue(
      modelState({
        options: [
          modelOption({ active: false, selected: false }),
          capable({ installed: true, active: true, selected: true, state: "active" }),
        ],
        activeId: "capable",
        selectedId: "capable",
      }),
    );

    render(<ModelSelector />);
    const localButton = await screen.findByTestId("choose-local-model-button");
    await userEvent.click(localButton);
    await waitFor(() => expect(commands.selectLocalModel).toHaveBeenCalled());
    expect(localButton).toHaveAccessibleName("Checking selected model file");
    expect(localButton).toHaveAttribute("aria-busy", "true");

    const trigger = screen.getByTestId("model-selector-trigger");
    expect(trigger).not.toBeDisabled();
    await userEvent.click(trigger);
    await userEvent.click(await screen.findByTestId("model-option-capable"));
    await waitFor(() => expect(trigger).toHaveTextContent("More capable"));
    expect(localButton).toHaveAccessibleName("Choose a model file");
    expect(localButton).toHaveAttribute("aria-busy", "false");

    const staleLocal = modelOption({
      id: "local-atlas",
      displayName: "atlas-4b",
      sourceKind: "local",
      localPath: "/Users/maya/Models/atlas-4b.gguf",
    });
    resolveLocal?.(
      modelState({ options: [staleLocal], activeId: "local-atlas", selectedId: "local-atlas" }),
    );
    await act(async () => Promise.resolve());

    expect(trigger).toHaveTextContent("More capable");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("More capable");
  });

  it("keeps a non-selected download visible and updates it from progress events", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ state: "downloading", bytesDownloaded: 500 })],
        downloads: [modelDownload({ bytesDownloaded: 500, bytesTotal: 1_000, revision: 4 })],
      }),
    );

    render(<ModelSelector />);

    expect(await screen.findByTestId("model-selector-trigger")).toHaveTextContent("Balanced");
    const row = screen.getByTestId("model-download-capable");
    expect(row).toHaveTextContent("More capable");
    expect(row).toHaveTextContent("50%");
    await waitFor(() => expect(progressEvents.callback).not.toBeNull());

    act(() => {
      progressEvents.callback?.(
        progressEvent({ bytesDownloaded: 750, bytesTotal: 1_000, revision: 5 }),
      );
    });

    expect(screen.getByTestId("model-selector-trigger")).toHaveTextContent("Balanced");
    expect(screen.getByTestId("model-download-capable")).toHaveTextContent("75%");
  });

  it("pauses a download without changing the selected or active model", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ state: "downloading" })],
        downloads: [modelDownload()],
      }),
    );
    vi.mocked(commands.pauseModelDownload).mockResolvedValue(
      modelDownload({ state: "paused", revision: 2 }),
    );

    render(<ModelSelector />);

    await userEvent.click(
      await screen.findByRole("button", { name: "Pause download of More capable" }),
    );

    await waitFor(() => expect(commands.pauseModelDownload).toHaveBeenCalledOnce());
    expect(commands.pauseModelDownload).toHaveBeenCalledWith("capable");
    const row = screen.getByTestId("model-download-capable");
    expect(row).toHaveTextContent("Paused at 50%");
    expect(
      within(row).getByRole("button", { name: "Resume download of More capable" }),
    ).toBeEnabled();
    expect(
      within(row).getByRole("button", { name: "Stop download of More capable" }),
    ).toBeEnabled();
    expect(
      within(row).queryByRole("button", { name: "Pause download of More capable" }),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("model-selector-trigger")).toHaveTextContent("Balanced");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
  });

  it("resumes a paused download from its existing byte count", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ state: "paused", bytesDownloaded: 1_000 })],
        downloads: [modelDownload({ state: "paused", revision: 3 })],
      }),
    );
    vi.mocked(commands.resumeModelDownload).mockResolvedValue(
      modelDownload({ state: "queued", revision: 4 }),
    );

    render(<ModelSelector />);

    await userEvent.click(
      await screen.findByRole("button", { name: "Resume download of More capable" }),
    );

    await waitFor(() => expect(commands.resumeModelDownload).toHaveBeenCalledOnce());
    expect(commands.resumeModelDownload).toHaveBeenCalledWith("capable");
    const row = screen.getByTestId("model-download-capable");
    expect(row).toHaveTextContent("Waiting…");
    expect(
      within(row).getByRole("button", { name: "Pause download of More capable" }),
    ).toBeEnabled();
    expect(screen.getAllByTestId("model-download-capable")).toHaveLength(1);

    await waitFor(() => expect(progressEvents.callback).not.toBeNull());
    act(() => {
      progressEvents.callback?.(progressEvent({ bytesDownloaded: 1_000, revision: 5 }));
    });
    expect(screen.getByTestId("model-download-capable")).toHaveTextContent("50%");
  });

  it("stops a download and removes only its standalone row", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ state: "downloading" })],
        downloads: [modelDownload()],
      }),
    );
    vi.mocked(commands.stopModelDownload).mockResolvedValue(
      modelDownload({ state: "stopped", bytesDownloaded: 0, revision: 2 }),
    );

    render(<ModelSelector />);

    await userEvent.click(
      await screen.findByRole("button", { name: "Stop download of More capable" }),
    );

    await waitFor(() => expect(commands.stopModelDownload).toHaveBeenCalledOnce());
    expect(commands.stopModelDownload).toHaveBeenCalledWith("capable");
    await waitFor(() =>
      expect(screen.queryByTestId("model-download-capable")).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId("model-selector-trigger")).toHaveTextContent("Balanced");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
  });

  it("coalesces repeated pause clicks while the first command is pending", async () => {
    let resolvePause: ((download: Download) => void) | undefined;
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ state: "downloading" })],
        downloads: [modelDownload()],
      }),
    );
    vi.mocked(commands.pauseModelDownload).mockImplementation(
      () =>
        new Promise<Download>((resolve) => {
          resolvePause = resolve;
        }),
    );
    const user = userEvent.setup();

    render(<ModelSelector />);

    const pause = await screen.findByRole("button", {
      name: "Pause download of More capable",
    });
    await user.click(pause);
    await waitFor(() => expect(commands.pauseModelDownload).toHaveBeenCalledOnce());
    expect(pause).toBeDisabled();
    await user.click(pause);
    expect(commands.pauseModelDownload).toHaveBeenCalledOnce();

    await act(async () => {
      resolvePause?.(modelDownload({ state: "paused", revision: 2 }));
    });
    expect(
      await screen.findByRole("button", { name: "Resume download of More capable" }),
    ).toBeEnabled();
  });

  it("ignores stale download events with a lower revision", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ state: "downloading", bytesDownloaded: 1_000 })],
        downloads: [modelDownload({ revision: 7 })],
      }),
    );

    render(<ModelSelector />);

    const row = await screen.findByTestId("model-download-capable");
    await waitFor(() => expect(progressEvents.callback).not.toBeNull());
    act(() => {
      progressEvents.callback?.(
        progressEvent({
          state: "paused",
          bytesDownloaded: 1_800,
          revision: 6,
        }),
      );
    });

    expect(row).toHaveTextContent("50%");
    expect(
      within(row).getByRole("button", { name: "Pause download of More capable" }),
    ).toBeEnabled();
    expect(
      within(row).queryByRole("button", { name: "Resume download of More capable" }),
    ).not.toBeInTheDocument();
  });

  it("does not regress a download within the same revision", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [modelOption(), capable({ state: "downloading", bytesDownloaded: 1_000 })],
        downloads: [modelDownload({ revision: 7 })],
      }),
    );

    render(<ModelSelector />);

    const row = await screen.findByTestId("model-download-capable");
    await waitFor(() => expect(progressEvents.callback).not.toBeNull());
    act(() => {
      progressEvents.callback?.(
        progressEvent({ state: "queued", bytesDownloaded: 1_000, revision: 7 }),
      );
      progressEvents.callback?.(
        progressEvent({ state: "downloading", bytesDownloaded: 500, revision: 7 }),
      );
    });

    expect(row).toHaveTextContent("50%");
    expect(row).not.toHaveTextContent("Waiting");
  });

  it("keeps newer live progress when re-selecting the same running job is a backend no-op", async () => {
    const initial = modelState({
      options: [modelOption(), capable({ state: "downloading", bytesDownloaded: 1_000 })],
      downloads: [modelDownload({ revision: 7 })],
    });
    const staleSelectionSnapshot = modelState({
      options: [
        modelOption({ selected: false }),
        capable({ selected: true, state: "queued", bytesDownloaded: 1_000 }),
      ],
      selectedId: "capable",
      downloads: [modelDownload({ state: "queued", revision: 7 })],
    });
    vi.mocked(commands.getModelState).mockResolvedValue(initial);
    vi.mocked(commands.selectCuratedModel).mockResolvedValue(staleSelectionSnapshot);

    render(<ModelSelector />);
    await screen.findByTestId("model-download-capable");
    await waitFor(() => expect(progressEvents.callback).not.toBeNull());
    act(() => {
      progressEvents.callback?.(
        progressEvent({ bytesDownloaded: 1_500, bytesTotal: 2_000, revision: 7 }),
      );
    });
    expect(screen.getByTestId("model-download-capable")).toHaveTextContent("75%");

    await userEvent.click(screen.getByTestId("model-selector-trigger"));
    await userEvent.click(await screen.findByTestId("model-option-capable"));

    await waitFor(() => expect(commands.selectCuratedModel).toHaveBeenCalledWith("capable"));
    expect(screen.getByTestId("model-selector-trigger")).toHaveTextContent("More capable");
    expect(screen.getByTestId("model-download-capable")).toHaveTextContent("75%");
    expect(screen.getByTestId("model-download-capable")).not.toHaveTextContent("Waiting");
  });

  it.each(["stopped", "completed"] as const)(
    "removes a download row after a %s event",
    async (state) => {
      vi.mocked(commands.getModelState).mockResolvedValue(
        modelState({
          options: [modelOption(), capable({ state: "downloading" })],
          downloads: [modelDownload({ revision: 2 })],
        }),
      );

      render(<ModelSelector />);

      await screen.findByTestId("model-download-capable");
      await waitFor(() => expect(progressEvents.callback).not.toBeNull());
      act(() => {
        progressEvents.callback?.(progressEvent({ state, bytesDownloaded: 2_000, revision: 3 }));
      });

      expect(screen.queryByTestId("model-download-capable")).not.toBeInTheDocument();
      expect(screen.getByTestId("model-selector-trigger")).toHaveTextContent("Balanced");
    },
  );

  it("shows indeterminate download progress without rendering NaN for an unknown total", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [
          modelOption({ selected: false }),
          capable({ selected: true, state: "downloading", bytesDownloaded: 0, bytesTotal: 0 }),
        ],
        selectedId: "capable",
        downloads: [modelDownload({ bytesDownloaded: 0, bytesTotal: 0 })],
      }),
    );

    const { container } = render(<ModelSelector />);

    expect(await screen.findByTestId("model-download-capable")).toHaveTextContent("Downloading…");
    expect(container.innerHTML).not.toContain("NaN");
  });

  it.each([
    ["verifying", "Verifying…", "Verifying More capable"],
    ["queued", "Waiting…", "Waiting to download More capable"],
  ] as const)(
    "shows the %s state instead of a stale 100%%",
    async (state, copy, accessibleName) => {
      vi.mocked(commands.getModelState).mockResolvedValue(
        modelState({
          options: [
            modelOption({ selected: false }),
            capable({
              selected: true,
              state,
              bytesDownloaded: 5_100_000_000,
              bytesTotal: 5_100_000_000,
            }),
          ],
          selectedId: "capable",
          downloads: [
            modelDownload({
              state,
              bytesDownloaded: 5_100_000_000,
              bytesTotal: 5_100_000_000,
            }),
          ],
        }),
      );

      render(<ModelSelector />);

      const progress = await screen.findByRole("progressbar", { name: accessibleName });
      expect(screen.getByTestId("model-download-capable")).toHaveTextContent(copy);
      expect(screen.getByTestId("model-download-capable")).not.toHaveTextContent("100%");
      expect(progress).not.toHaveAttribute("aria-valuenow");
    },
  );

  it("opens a GGUF-only native picker and passes the selected path to the backend", async () => {
    vi.mocked(open).mockResolvedValue("/Users/maya/Models/atlas-4b.gguf");
    const local = modelOption({
      id: "local-atlas",
      displayName: "atlas-4b",
      description: "A compatible model file from this Mac.",
      technicalName: "atlas-4b.gguf",
      parameterCount: "Local",
      quantization: "GGUF",
      sizeBytes: 2_600_000_000,
      recommended: false,
      sourceKind: "local",
      localPath: "/Users/maya/Models/atlas-4b.gguf",
    });
    vi.mocked(commands.selectLocalModel).mockResolvedValue(
      modelState({
        options: [modelOption({ active: false, selected: false }), local],
        activeId: "local-atlas",
        selectedId: "local-atlas",
      }),
    );

    render(<ModelSelector />);
    await userEvent.click(await screen.findByTestId("choose-local-model-button"));

    await waitFor(() => {
      expect(open).toHaveBeenCalledWith({
        multiple: false,
        directory: false,
        filters: [{ name: "GGUF model", extensions: ["gguf"] }],
      });
      expect(commands.selectLocalModel).toHaveBeenCalledWith("/Users/maya/Models/atlas-4b.gguf");
    });
    expect(screen.getByTestId("model-selector-trigger")).toHaveTextContent("atlas-4b");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("atlas-4b");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Active");
    await userEvent.click(screen.getByTestId("model-selector-trigger"));
    expect(await screen.findByTestId("model-option-local-atlas")).toHaveAccessibleName(
      /atlas-4b.*Users\/maya\/Models\/atlas-4b\.gguf/,
    );
  });

  it("treats cancelling the local picker as a no-op", async () => {
    vi.mocked(open).mockResolvedValue(null);

    render(<ModelSelector />);
    await userEvent.click(await screen.findByTestId("choose-local-model-button"));

    expect(commands.selectLocalModel).not.toHaveBeenCalled();
    expect(screen.queryByTestId("model-action-error")).not.toBeInTheDocument();
  });

  it("shows local validation errors without replacing the active model", async () => {
    vi.mocked(open).mockResolvedValue("/Users/maya/Models/broken.gguf");
    vi.mocked(commands.selectLocalModel).mockRejectedValue(
      new Error("The selected file is not a valid GGUF model."),
    );

    render(<ModelSelector />);
    await userEvent.click(await screen.findByTestId("choose-local-model-button"));

    expect(await screen.findByTestId("model-action-error")).toHaveTextContent(
      "not a valid GGUF model",
    );
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
  });

  it("keeps the active model visible after an error and offers retry", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [
          modelOption({ selected: false }),
          capable({ selected: true, state: "error: Not enough disk space" }),
        ],
        selectedId: "capable",
      }),
    );
    vi.mocked(commands.selectCuratedModel).mockResolvedValue(
      modelState({ options: [modelOption(), capable()] }),
    );

    render(<ModelSelector />);

    expect(await screen.findByTestId("model-error")).toHaveTextContent("Not enough disk space");
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("Balanced");
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(commands.selectCuratedModel).toHaveBeenCalledWith("capable");
  });

  it("shows and dismisses a recovered local-model fallback notice", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        fallbackNotice: "atlas-4b.gguf could not be found, so Doce switched back to Balanced.",
      }),
    );
    vi.mocked(commands.dismissModelNotice).mockResolvedValue(undefined);

    render(<ModelSelector />);

    expect(await screen.findByTestId("model-fallback-notice")).toHaveTextContent(
      "switched back to Balanced",
    );
    await userEvent.click(screen.getByRole("button", { name: "Dismiss model notice" }));
    await waitFor(() => expect(commands.dismissModelNotice).toHaveBeenCalledOnce());
    expect(screen.queryByTestId("model-fallback-notice")).not.toBeInTheDocument();
  });

  it("uses recovery language and a loading status while a fallback has no active model", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({
        options: [
          modelOption({
            active: false,
            selected: true,
            installed: false,
            state: "downloading",
            bytesDownloaded: 500,
            bytesTotal: 1_000,
          }),
        ],
        activeId: null,
        selectedId: "balanced",
        fallbackNotice: "The local model is no longer available. Doce is getting Balanced ready.",
        downloads: [
          modelDownload({
            modelId: "balanced",
            displayName: "Balanced",
            bytesDownloaded: 500,
            bytesTotal: 1_000,
          }),
        ],
      }),
    );

    render(<ModelSelector />);

    expect(await screen.findByTestId("model-fallback-notice")).toHaveTextContent(
      "Doce is getting Balanced ready",
    );
    expect(screen.getByTestId("active-model-summary")).toHaveTextContent("No active model");
    expect(screen.getByTestId("model-download-balanced")).toHaveTextContent("50%");
  });

  it("associates the visible model label and description with the selector", async () => {
    render(<ModelSelector />);

    const trigger = await screen.findByTestId("model-selector-trigger");
    expect(trigger).toHaveAccessibleName("AI model Balanced");
    expect(trigger).toHaveAccessibleDescription(
      "Used for every conversation and task. Doce downloads it when needed.",
    );
  });

  it("announces that the active model is loading before the first snapshot resolves", () => {
    vi.mocked(commands.getModelState).mockReturnValue(new Promise(() => undefined));

    render(<ModelSelector />);

    expect(screen.getByTestId("active-model-summary")).toHaveTextContent(
      "Checking the active model…",
    );
  });

  const endpointOption = (overrides: Partial<Option> = {}) =>
    modelOption({
      id: "endpoint:openrouter",
      displayName: "qwen-max",
      description: "",
      technicalName: "",
      recommended: false,
      installed: true,
      active: false,
      selected: false,
      sourceKind: "endpoint",
      endpointUrl: "https://openrouter.ai/api/v1",
      endpointModel: "qwen-max",
      state: "ready",
      ...overrides,
    });

  it("renders existing endpoints as their own group with model and host", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({ options: [modelOption(), endpointOption()] }),
    );

    render(<ModelSelector />);
    await userEvent.click(await screen.findByTestId("model-selector-trigger"));

    const row = await screen.findByTestId("model-option-endpoint:openrouter");
    expect(row).toHaveTextContent("qwen-max");
    expect(row).toHaveTextContent("openrouter.ai");
    expect(screen.getByText("Endpoints")).toBeVisible();
  });

  it("re-opens an existing endpoint pre-filled instead of re-selecting it", async () => {
    vi.mocked(commands.getModelState).mockResolvedValue(
      modelState({ options: [modelOption(), endpointOption()] }),
    );

    render(<ModelSelector />);
    await userEvent.click(await screen.findByTestId("model-selector-trigger"));
    await userEvent.click(await screen.findByTestId("model-option-endpoint:openrouter"));

    const form = await screen.findByTestId("add-endpoint-form");
    expect(within(form).getByTestId("endpoint-url-input")).toHaveValue(
      "https://openrouter.ai/api/v1",
    );
    expect(commands.selectCuratedModel).not.toHaveBeenCalled();
  });

  it("opens an empty endpoint form from the Add button", async () => {
    render(<ModelSelector />);

    await userEvent.click(await screen.findByTestId("add-endpoint-button"));
    const form = await screen.findByTestId("add-endpoint-form");
    expect(within(form).getByTestId("endpoint-url-input")).toHaveValue("");
  });

  it("unsubscribes from model progress events when unmounted", async () => {
    const { unmount } = render(<ModelSelector />);
    await screen.findByTestId("model-selector-trigger");
    await waitFor(() => expect(progressEvents.callback).not.toBeNull());

    unmount();
    expect(progressEvents.unlisten).toHaveBeenCalledOnce();
  });
});
