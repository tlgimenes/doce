import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  CheckCircle2,
  ChevronsUpDown,
  FileUp,
  Info,
  Pause,
  Play,
  RotateCcw,
  Square,
  X,
} from "lucide-react";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Progress, ProgressLabel } from "@/components/ui/progress";
import { Spinner } from "@/components/ui/spinner";
import { commands, events, type ModelDownload, type ModelOption, type ModelState } from "@/lib/ipc";
import { pathBasename } from "@/lib/pathBasename";

const VISIBLE_DOWNLOAD_STATES = new Set(["queued", "downloading", "verifying", "paused", "failed"]);
// A backend revision identifies one writer attempt or control transition;
// queued/chunk/verifying events within that attempt intentionally share it.
// This phase order and the byte comparison below keep a delayed command
// snapshot from regressing newer live progress at the same revision.
const DOWNLOAD_STATE_ORDER: Record<string, number> = {
  queued: 0,
  downloading: 1,
  verifying: 2,
  downloaded: 3,
  completed: 3,
  installed: 4,
  ready: 4,
  preparing: 5,
  paused: 6,
  stopped: 6,
  failed: 6,
  active: 7,
};

interface DownloadVersion {
  revision: number;
  state: string;
  bytesDownloaded: number;
}

interface DownloadVersionUpdate extends DownloadVersion {
  modelId: string;
}

function downloadVersion(update: DownloadVersionUpdate): DownloadVersion {
  return {
    revision: update.revision ?? 0,
    state: update.state,
    bytesDownloaded: update.bytesDownloaded,
  };
}

function shouldApplyDownloadVersion(
  current: DownloadVersion | undefined,
  incoming: DownloadVersion,
): boolean {
  if (!current) return true;
  if (incoming.revision !== current.revision) return incoming.revision > current.revision;
  if (incoming.state === current.state) {
    return incoming.bytesDownloaded >= current.bytesDownloaded;
  }
  const currentOrder = current.state.startsWith("error") ? 6 : DOWNLOAD_STATE_ORDER[current.state];
  const incomingOrder = incoming.state.startsWith("error")
    ? 6
    : DOWNLOAD_STATE_ORDER[incoming.state];
  if (currentOrder === undefined || incomingOrder === undefined) return true;
  return incomingOrder >= currentOrder;
}

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) return error.message;
  return typeof error === "string" && error ? error : fallback;
}

function modelName(option: ModelOption | undefined): string {
  if (!option) return "No active model";
  if (option.displayName.trim()) return option.displayName;
  if (option.localPath) return pathBasename(option.localPath);
  return option.technicalName || option.id;
}

function downloadPercent(download: Pick<ModelDownload, "bytesDownloaded" | "bytesTotal">) {
  if (!download.bytesTotal) return null;
  return Math.min(
    100,
    Math.max(0, Math.round((download.bytesDownloaded / download.bytesTotal) * 100)),
  );
}

function downloadCopy(download: ModelDownload): string {
  if (download.state === "queued") return `Waiting to download ${download.displayName}`;
  if (download.state === "verifying") return `Verifying ${download.displayName}`;
  if (download.state === "paused") return `${download.displayName} download paused`;
  if (download.state === "failed") return `Couldn’t download ${download.displayName}`;
  return `Downloading ${download.displayName}`;
}

function downloadValueCopy(download: ModelDownload, percent: number | null): string {
  if (download.state === "queued") return "Waiting…";
  if (download.state === "verifying") return "Verifying…";
  if (download.state === "paused") return percent === null ? "Paused" : `Paused at ${percent}%`;
  if (download.state === "failed") return "Needs attention";
  return percent === null ? "Downloading…" : `${percent}%`;
}

function stateError(state: string): string | null {
  if (!state.startsWith("error")) return null;
  const detail = state.replace(/^error:?\s*/i, "").trim();
  return detail || "Doce couldn’t prepare this model.";
}

function optionAvailabilityCopy(option: ModelOption, download: ModelDownload | undefined): string {
  if (download?.state === "queued") return "Waiting to download";
  if (download?.state === "downloading") {
    const percent = downloadPercent(download);
    return percent === null ? "Downloading" : `Downloading · ${percent}%`;
  }
  if (download?.state === "verifying") return "Verifying download";
  if (download?.state === "paused") return "Download paused";
  if (download?.state === "failed") return "Download needs attention";
  if (option.state === "stopped") return "Download stopped · select to try again";
  return option.installed ? "Ready to use" : "Downloads when selected";
}

export default function ModelSelector() {
  const [modelState, setModelState] = useState<ModelState | null>(null);
  const [loading, setLoading] = useState(true);
  const [requesting, setRequesting] = useState(false);
  const [choosingLocal, setChoosingLocal] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [requestedId, setRequestedId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [downloadActions, setDownloadActions] = useState<
    Record<string, "pause" | "resume" | "stop">
  >({});
  const [downloadErrors, setDownloadErrors] = useState<Record<string, string>>({});
  const selectedIdRef = useRef<string | null>(null);
  const requestSequenceRef = useRef(0);
  const downloadVersionsRef = useRef(new Map<string, DownloadVersion>());
  const downloadActionsRef = useRef(new Set<string>());

  const acceptDownloadUpdate = useCallback((update: DownloadVersionUpdate) => {
    const incoming = downloadVersion(update);
    const current = downloadVersionsRef.current.get(update.modelId);
    if (!shouldApplyDownloadVersion(current, incoming)) return false;
    downloadVersionsRef.current.set(update.modelId, incoming);
    return true;
  }, []);

  const applySnapshot = useCallback(
    (next: ModelState) => {
      const incomingDownloads = next.downloads ?? [];
      const normalized = {
        ...next,
        downloads: incomingDownloads.filter((download) =>
          VISIBLE_DOWNLOAD_STATES.has(download.state),
        ),
      };
      setModelState((current) => {
        const currentById = new Map(
          (current?.downloads ?? []).map((download) => [download.modelId, download]),
        );
        const downloads: ModelDownload[] = [];
        const seen = new Set<string>();
        for (const incoming of incomingDownloads) {
          seen.add(incoming.modelId);
          if (acceptDownloadUpdate(incoming)) {
            if (VISIBLE_DOWNLOAD_STATES.has(incoming.state)) downloads.push(incoming);
          } else {
            const existing = currentById.get(incoming.modelId);
            if (existing) downloads.push(existing);
          }
        }
        for (const existing of current?.downloads ?? []) {
          if (!seen.has(existing.modelId)) downloads.push(existing);
        }
        const downloadById = new Map(downloads.map((download) => [download.modelId, download]));
        return {
          ...next,
          downloads,
          options: next.options.map((option) => {
            const download = downloadById.get(option.id);
            return download
              ? {
                  ...option,
                  state: download.state,
                  bytesDownloaded: download.bytesDownloaded,
                  bytesTotal: download.bytesTotal,
                }
              : option;
          }),
        };
      });
      const nextSelectedId = next.selectedId ?? next.activeId;
      selectedIdRef.current = nextSelectedId;
      setRequestedId(null);
      return normalized;
    },
    [acceptDownloadUpdate],
  );

  const applyDownloadUpdate = useCallback(
    (nextDownload: ModelDownload) => {
      if (!acceptDownloadUpdate(nextDownload)) return false;
      setModelState((current) => {
        if (!current) return current;
        const existing = current.downloads.find(
          (download) => download.modelId === nextDownload.modelId,
        );
        const downloads = VISIBLE_DOWNLOAD_STATES.has(nextDownload.state)
          ? existing
            ? current.downloads.map((download) =>
                download.modelId === nextDownload.modelId ? nextDownload : download,
              )
            : [...current.downloads, nextDownload]
          : current.downloads.filter((download) => download.modelId !== nextDownload.modelId);
        return {
          ...current,
          downloads,
          options: current.options.map((option) =>
            option.id === nextDownload.modelId
              ? {
                  ...option,
                  state:
                    nextDownload.state === "failed"
                      ? `error: ${nextDownload.error ?? "Download failed"}`
                      : nextDownload.state,
                  bytesDownloaded: nextDownload.bytesDownloaded,
                  bytesTotal: nextDownload.bytesTotal,
                }
              : option,
          ),
        };
      });
      return true;
    },
    [acceptDownloadUpdate],
  );

  const refresh = useCallback(async () => {
    const next = await commands.getModelState();
    return applySnapshot(next);
  }, [applySnapshot]);

  useEffect(() => {
    let cancelled = false;
    refresh()
      .catch((error) => {
        if (!cancelled) {
          setActionError(errorMessage(error, "Doce couldn’t load the available models."));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    events
      .onModelInstallProgress((nextProgress) => {
        if (!acceptDownloadUpdate(nextProgress)) return;
        const revision = nextProgress.revision ?? 0;
        const becameActive = nextProgress.state === "active";
        const commitsCurrentSelection =
          becameActive && selectedIdRef.current === nextProgress.modelId;
        setModelState((current) => {
          if (!current) return current;
          const becameInstalled = ["completed", "installed", "active"].includes(nextProgress.state);
          const currentDownload = current.downloads.find(
            (download) => download.modelId === nextProgress.modelId,
          );
          const option = current.options.find((candidate) => candidate.id === nextProgress.modelId);
          const nextDownload: ModelDownload = {
            modelId: nextProgress.modelId,
            displayName: currentDownload?.displayName ?? modelName(option),
            state: nextProgress.state as ModelDownload["state"],
            bytesDownloaded: nextProgress.bytesDownloaded,
            bytesTotal: nextProgress.bytesTotal,
            revision,
            error: nextProgress.error ?? null,
          };
          const downloads = VISIBLE_DOWNLOAD_STATES.has(nextProgress.state)
            ? currentDownload
              ? current.downloads.map((download) =>
                  download.modelId === nextProgress.modelId ? nextDownload : download,
                )
              : [...current.downloads, nextDownload]
            : current.downloads.filter((download) => download.modelId !== nextProgress.modelId);
          return {
            ...current,
            downloads,
            activeId: becameActive ? nextProgress.modelId : current.activeId,
            selectedId: commitsCurrentSelection ? nextProgress.modelId : current.selectedId,
            options: current.options.map((option) => {
              if (option.id === nextProgress.modelId) {
                return {
                  ...option,
                  installed: becameInstalled || option.installed,
                  active: becameActive || option.active,
                  selected: commitsCurrentSelection || option.selected,
                  state:
                    nextProgress.state === "failed"
                      ? `error: ${nextProgress.error ?? "Download failed"}`
                      : nextProgress.state,
                  bytesDownloaded: nextProgress.bytesDownloaded,
                  bytesTotal: nextProgress.bytesTotal,
                };
              }
              return becameActive
                ? {
                    ...option,
                    active: false,
                    selected: commitsCurrentSelection ? false : option.selected,
                  }
                : option;
            }),
          };
        });
        if (commitsCurrentSelection) {
          selectedIdRef.current = nextProgress.modelId;
          setRequestedId(null);
          setRequesting(false);
          const activeEventSequence = requestSequenceRef.current;
          void commands
            .getModelState()
            .then((next) => {
              if (
                activeEventSequence === requestSequenceRef.current &&
                selectedIdRef.current === nextProgress.modelId
              ) {
                applySnapshot(next);
              }
            })
            .catch(() => {
              // The committed event already updated activeId and the option
              // flags above, so a transient snapshot failure cannot leave the
              // screen stuck in a false downloading state.
            });
        }
      })
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else unlisten = unsubscribe;
      })
      .catch(() => {
        // The snapshot remains usable if live events are unavailable.
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [acceptDownloadUpdate, applySnapshot]);

  const options = modelState?.options ?? [];
  const activeOption = options.find(
    (option) => option.id === modelState?.activeId || option.active,
  );
  const selectedId = requestedId ?? modelState?.selectedId ?? modelState?.activeId ?? "";
  const selectedOption = options.find((option) => option.id === selectedId);
  const pendingSelectionOption = requesting && requestedId ? selectedOption : undefined;
  const curatedOptions = options.filter((option) => option.sourceKind === "curated");
  const localOptions = options.filter((option) => option.sourceKind === "local");
  const downloads = modelState?.downloads ?? [];
  const selectedDownload = downloads.find((download) => download.modelId === selectedOption?.id);
  const pendingError =
    selectedOption && selectedOption.id !== modelState?.activeId && !selectedDownload
      ? stateError(selectedOption.state)
      : null;

  const chooseOption = async (option: ModelOption, retry = false) => {
    if (!retry && option.id === selectedId) return;
    const requestSequence = ++requestSequenceRef.current;
    setRequesting(true);
    setChoosingLocal(false);
    setActionError(null);
    setRequestedId(option.id);
    selectedIdRef.current = option.id;
    try {
      let next: ModelState;
      if (option.sourceKind === "local" && option.localPath) {
        next = await commands.selectLocalModel(option.localPath);
      } else {
        next = await commands.selectCuratedModel(option.id);
      }
      if (requestSequence !== requestSequenceRef.current) return;
      applySnapshot(next);
    } catch (error) {
      if (requestSequence !== requestSequenceRef.current) return;
      setRequestedId(null);
      selectedIdRef.current = modelState?.selectedId ?? modelState?.activeId ?? null;
      setActionError(errorMessage(error, "Doce couldn’t select this model."));
    } finally {
      if (requestSequence === requestSequenceRef.current) setRequesting(false);
    }
  };

  const controlDownload = async (download: ModelDownload, action: "pause" | "resume" | "stop") => {
    if (downloadActionsRef.current.has(download.modelId)) return;
    downloadActionsRef.current.add(download.modelId);
    setDownloadActions((current) => ({ ...current, [download.modelId]: action }));
    setDownloadErrors((current) => {
      const next = { ...current };
      delete next[download.modelId];
      return next;
    });
    try {
      const next =
        action === "pause"
          ? await commands.pauseModelDownload(download.modelId)
          : action === "resume"
            ? await commands.resumeModelDownload(download.modelId)
            : await commands.stopModelDownload(download.modelId);
      applyDownloadUpdate(next);
    } catch (error) {
      setDownloadErrors((current) => ({
        ...current,
        [download.modelId]: errorMessage(
          error,
          action === "pause"
            ? "Doce couldn’t pause this download."
            : action === "resume"
              ? "Doce couldn’t resume this download."
              : "Doce couldn’t stop this download.",
        ),
      }));
    } finally {
      downloadActionsRef.current.delete(download.modelId);
      setDownloadActions((current) => {
        if (current[download.modelId] !== action) return current;
        const next = { ...current };
        delete next[download.modelId];
        return next;
      });
    }
  };

  const chooseLocalFile = async () => {
    setActionError(null);
    let requestSequence: number | null = null;
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "GGUF model", extensions: ["gguf"] }],
      });
      if (!selected) return;
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (!path) return;

      requestSequence = ++requestSequenceRef.current;
      setRequesting(false);
      setChoosingLocal(true);
      const next = await commands.selectLocalModel(path);
      if (requestSequence === requestSequenceRef.current) applySnapshot(next);
    } catch (error) {
      if (requestSequence !== null && requestSequence !== requestSequenceRef.current) return;
      setActionError(errorMessage(error, "Doce couldn’t use this model file."));
    } finally {
      if (requestSequence === null || requestSequence === requestSequenceRef.current) {
        setChoosingLocal(false);
      }
    }
  };

  const dismissFallbackNotice = async () => {
    try {
      await commands.dismissModelNotice();
      setModelState((current) => (current ? { ...current, fallbackNotice: null } : current));
    } catch (error) {
      setActionError(errorMessage(error, "Doce couldn’t dismiss this notice."));
    }
  };

  return (
    <section
      className="mb-8"
      aria-labelledby="model-settings-heading"
      data-testid="settings-model-section"
    >
      <h3
        id="model-settings-heading"
        className="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase"
      >
        Model
      </h3>

      {modelState?.fallbackNotice ? (
        <Alert className="mb-3" data-testid="model-fallback-notice">
          <Info />
          <AlertTitle>
            {activeOption ? "Doce switched models" : "Doce is getting a model ready"}
          </AlertTitle>
          <AlertDescription>{modelState.fallbackNotice}</AlertDescription>
          <AlertAction>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Dismiss model notice"
              onClick={() => void dismissFallbackNotice()}
            >
              <X />
            </Button>
          </AlertAction>
        </Alert>
      ) : null}

      <Card size="sm" data-testid="settings-model-panel">
        <CardContent className="space-y-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0">
              <p id="model-selector-label" className="font-medium">
                AI model
              </p>
              <p id="model-selector-description" className="text-sm text-muted-foreground">
                Used for every conversation and task. Doce downloads it when needed.
              </p>
            </div>

            <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen} modal={false}>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    className="w-full min-w-0 justify-between sm:w-64"
                    disabled={loading}
                    aria-labelledby="model-selector-label model-selector-value"
                    aria-describedby="model-selector-description"
                    data-testid="model-selector-trigger"
                  />
                }
              >
                <span id="model-selector-value" className="truncate">
                  {loading ? "Loading models…" : modelName(selectedOption ?? activeOption)}
                </span>
                <ChevronsUpDown className="ml-1" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-80">
                <DropdownMenuRadioGroup
                  value={selectedId}
                  onValueChange={(value) => {
                    const option = options.find((candidate) => candidate.id === value);
                    if (option) {
                      setMenuOpen(false);
                      void chooseOption(option);
                    }
                  }}
                >
                  <DropdownMenuLabel>Curated by Doce</DropdownMenuLabel>
                  {curatedOptions.map((option) => {
                    const download = downloads.find((item) => item.modelId === option.id);
                    return (
                      <DropdownMenuRadioItem
                        key={option.id}
                        value={option.id}
                        className="items-start py-2"
                        data-testid={`model-option-${option.id}`}
                        onClick={() => {
                          if (option.id === selectedId && option.state === "stopped") {
                            setMenuOpen(false);
                            void chooseOption(option, true);
                          }
                        }}
                      >
                        <span className="min-w-0 flex-1">
                          <span className="flex min-w-0 items-center gap-2">
                            <span className="truncate font-medium">{modelName(option)}</span>
                            {option.recommended ? (
                              <Badge variant="secondary" className="h-4 px-1.5 text-[10px]">
                                Recommended
                              </Badge>
                            ) : null}
                          </span>
                          <span className="mt-0.5 block text-xs text-muted-foreground">
                            {option.description}
                          </span>
                          <span className="mt-0.5 block text-xs text-muted-foreground/80">
                            {optionAvailabilityCopy(option, download)}
                          </span>
                        </span>
                      </DropdownMenuRadioItem>
                    );
                  })}

                  {localOptions.length > 0 ? (
                    <>
                      <DropdownMenuSeparator />
                      <DropdownMenuLabel>From this Mac</DropdownMenuLabel>
                      {localOptions.map((option) => (
                        <DropdownMenuRadioItem
                          key={option.id}
                          value={option.id}
                          className="items-start py-2"
                          data-testid={`model-option-${option.id}`}
                          title={option.localPath ?? undefined}
                        >
                          <span className="min-w-0 flex-1">
                            <span className="block truncate font-medium">{modelName(option)}</span>
                            {option.localPath ? (
                              <span className="sr-only">, {option.localPath}</span>
                            ) : null}
                          </span>
                        </DropdownMenuRadioItem>
                      ))}
                    </>
                  ) : null}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          <div
            className="flex items-start gap-2.5 rounded-lg bg-muted/50 px-3 py-2.5"
            data-testid="active-model-summary"
            role="status"
            aria-live="polite"
            aria-atomic="true"
          >
            {loading || !activeOption ? (
              <Spinner className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
            ) : (
              <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-primary" />
            )}
            <div className="min-w-0">
              {loading ? (
                <span className="text-sm text-muted-foreground">Checking the active model…</span>
              ) : activeOption ? (
                <>
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">{modelName(activeOption)}</span>
                    <Badge variant="outline">Active</Badge>
                  </div>
                  {pendingSelectionOption && pendingSelectionOption.id !== activeOption.id ? (
                    <p
                      className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground"
                      data-testid="model-switch-status"
                    >
                      <Spinner className="size-3" />
                      {pendingSelectionOption.installed
                        ? `Switching to ${modelName(pendingSelectionOption)}…`
                        : `Starting the ${modelName(pendingSelectionOption)} download…`}
                    </p>
                  ) : (
                    <p className="mt-0.5 text-xs text-muted-foreground">
                      {activeOption.description}
                    </p>
                  )}
                  {activeOption.sourceKind === "local" && activeOption.localPath ? (
                    <p className="mt-0.5 truncate text-xs text-muted-foreground">
                      {activeOption.localPath}
                    </p>
                  ) : null}
                </>
              ) : (
                <>
                  <span className="font-medium">No active model</span>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    Doce is preparing the selected model for first use.
                  </p>
                </>
              )}
            </div>
          </div>

          {downloads.length > 0 ? (
            <div className="space-y-3 border-t pt-3" data-testid="model-downloads">
              <div>
                <p className="text-sm font-medium">Model downloads</p>
                <p className="text-xs text-muted-foreground">
                  Downloads continue if you choose a different model.
                </p>
              </div>

              <div className="space-y-2.5">
                {downloads.map((download) => {
                  const percent = downloadPercent(download);
                  const isDeterminate =
                    percent !== null && !["queued", "verifying"].includes(download.state);
                  const action = downloadActions[download.modelId];
                  const canPause = ["queued", "downloading", "verifying"].includes(download.state);
                  const canResume = ["paused", "failed"].includes(download.state);
                  const valueCopy = downloadValueCopy(download, percent);
                  const name = download.displayName || download.modelId;
                  const rowError = downloadErrors[download.modelId] ?? download.error;

                  return (
                    <div
                      key={download.modelId}
                      className="rounded-lg border bg-background px-3 py-2.5"
                      data-testid={`model-download-${download.modelId}`}
                      role="status"
                      aria-live="polite"
                    >
                      <Progress
                        value={isDeterminate ? percent : null}
                        aria-hidden="true"
                        className="gap-2"
                      >
                        <ProgressLabel className="flex min-w-0 items-center gap-1.5 text-xs">
                          {download.state === "downloading" || download.state === "verifying" ? (
                            <Spinner className="size-3.5 shrink-0" />
                          ) : null}
                          <span className="truncate">{downloadCopy(download)}</span>
                        </ProgressLabel>
                        <span className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums">
                          {valueCopy}
                        </span>
                      </Progress>
                      <span
                        className="sr-only"
                        role="progressbar"
                        aria-label={downloadCopy(download)}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-valuenow={isDeterminate ? (percent ?? undefined) : undefined}
                        aria-valuetext={valueCopy}
                      />

                      <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
                        <p className="min-w-0 flex-1 text-xs text-muted-foreground">
                          {rowError ??
                            (activeOption
                              ? `${modelName(activeOption)} stays active while this finishes.`
                              : "Doce will use this model when it is ready.")}
                        </p>
                        <div className="flex shrink-0 items-center gap-1">
                          {canPause ? (
                            <Button
                              variant="ghost"
                              size="xs"
                              onClick={() => void controlDownload(download, "pause")}
                              disabled={Boolean(action)}
                              aria-label={`Pause download of ${name}`}
                            >
                              {action === "pause" ? <Spinner /> : <Pause />}
                              Pause
                            </Button>
                          ) : null}
                          {canResume ? (
                            <Button
                              variant="ghost"
                              size="xs"
                              onClick={() => void controlDownload(download, "resume")}
                              disabled={Boolean(action)}
                              aria-label={`Resume download of ${name}`}
                            >
                              {action === "resume" ? <Spinner /> : <Play />}
                              Resume
                            </Button>
                          ) : null}
                          <Button
                            variant="ghost"
                            size="xs"
                            onClick={() => void controlDownload(download, "stop")}
                            disabled={Boolean(action)}
                            aria-label={`Stop download of ${name}`}
                          >
                            {action === "stop" ? <Spinner /> : <Square />}
                            Stop
                          </Button>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}

          <div className="flex flex-col gap-3 border-t pt-4 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0">
              <p className="font-medium">Model from this Mac</p>
              <p className="text-sm text-muted-foreground">
                Choose a compatible model file. Doce checks it before switching.
              </p>
            </div>
            <Button
              variant="outline"
              size="sm"
              className="shrink-0"
              onClick={() => void chooseLocalFile()}
              disabled={choosingLocal}
              data-testid="choose-local-model-button"
              aria-label={choosingLocal ? "Checking selected model file" : "Choose a model file"}
              aria-busy={choosingLocal}
            >
              {choosingLocal ? <Spinner /> : <FileUp />}
              {choosingLocal ? "Checking…" : "Choose…"}
            </Button>
          </div>
        </CardContent>
      </Card>

      {pendingError ? (
        <Alert className="mt-2" variant="destructive" data-testid="model-error">
          <AlertTitle>Couldn’t switch models</AlertTitle>
          <AlertDescription>
            {pendingError} {activeOption ? `${modelName(activeOption)} is still active.` : null}
          </AlertDescription>
          <AlertAction>
            <Button
              variant="ghost"
              size="xs"
              onClick={() => selectedOption && void chooseOption(selectedOption, true)}
              disabled={requesting}
            >
              <RotateCcw />
              Retry
            </Button>
          </AlertAction>
        </Alert>
      ) : null}

      {actionError ? (
        <Alert className="mt-2" variant="destructive" data-testid="model-action-error">
          <AlertTitle>Model unchanged</AlertTitle>
          <AlertDescription>
            {actionError} {activeOption ? `${modelName(activeOption)} is still active.` : null}
          </AlertDescription>
        </Alert>
      ) : null}
    </section>
  );
}
