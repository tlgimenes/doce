import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { CheckCircle2, ChevronsUpDown, FileUp, Info, RotateCcw, Sparkles, X } from "lucide-react";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
import {
  commands,
  events,
  type ModelInstallProgressPayload,
  type ModelOption,
  type ModelState,
} from "@/lib/ipc";
import { pathBasename } from "@/lib/pathBasename";

const BUSY_STATES = new Set(["queued", "downloading", "verifying", "preparing"]);

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

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "Size unavailable";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const unitIndex = Math.min(Math.floor(Math.log(bytes) / Math.log(1000)), units.length - 1);
  const value = bytes / 1000 ** unitIndex;
  const digits = unitIndex >= 3 && value < 10 ? 1 : 0;
  return `${value.toFixed(digits)} ${units[unitIndex]}`;
}

function formatParameters(value: ModelOption["parameterCount"]): string {
  if (typeof value === "number") {
    if (value >= 1_000_000_000)
      return `${(value / 1_000_000_000).toFixed(1).replace(/\.0$/, "")}B parameters`;
    if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(0)}M parameters`;
    return `${value} parameters`;
  }
  if (!value) return "";
  return /parameters?/i.test(value) ? value : `${value} parameters`;
}

function progressCopy(state: string, name: string): string {
  if (state === "active") return `${name} is ready`;
  if (["available", "idle", "installed", "queued", "ready"].includes(state)) {
    return `Waiting to switch to ${name}`;
  }
  if (state === "verifying") return `Verifying ${name}`;
  if (state === "preparing") return `Preparing ${name}`;
  return `Downloading ${name}`;
}

function stateError(state: string): string | null {
  if (!state.startsWith("error")) return null;
  const detail = state.replace(/^error:?\s*/i, "").trim();
  return detail || "Doce couldn’t prepare this model.";
}

export default function ModelSelector() {
  const [modelState, setModelState] = useState<ModelState | null>(null);
  const [loading, setLoading] = useState(true);
  const [requesting, setRequesting] = useState(false);
  const [choosingLocal, setChoosingLocal] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [requestedId, setRequestedId] = useState<string | null>(null);
  const [progress, setProgress] = useState<ModelInstallProgressPayload | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const selectedIdRef = useRef<string | null>(null);
  const requestSequenceRef = useRef(0);

  const applySnapshot = useCallback((next: ModelState) => {
    setModelState(next);
    selectedIdRef.current = next.selectedId ?? next.activeId;
    setRequestedId(null);
    return next;
  }, []);

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
        if (nextProgress.modelId !== selectedIdRef.current) return;
        setProgress(nextProgress);
        setModelState((current) => {
          if (!current) return current;
          const becameActive = nextProgress.state === "active";
          return {
            ...current,
            activeId: becameActive ? nextProgress.modelId : current.activeId,
            selectedId: becameActive ? nextProgress.modelId : current.selectedId,
            options: current.options.map((option) => {
              if (option.id === nextProgress.modelId) {
                return {
                  ...option,
                  installed: becameActive || option.installed,
                  active: becameActive || option.active,
                  selected: becameActive || option.selected,
                  state: nextProgress.state,
                  bytesDownloaded: nextProgress.bytesDownloaded,
                  bytesTotal: nextProgress.bytesTotal,
                };
              }
              return becameActive ? { ...option, active: false, selected: false } : option;
            }),
          };
        });
        if (nextProgress.state === "active") {
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
  }, [applySnapshot]);

  const options = modelState?.options ?? [];
  const activeOption = options.find(
    (option) => option.id === modelState?.activeId || option.active,
  );
  const selectedId = requestedId ?? modelState?.selectedId ?? modelState?.activeId ?? "";
  const selectedOption = options.find((option) => option.id === selectedId);
  const recommendedOption = options.find(
    (option) => option.sourceKind === "curated" && option.recommended,
  );
  const curatedOptions = options.filter((option) => option.sourceKind === "curated");
  const localOptions = options.filter((option) => option.sourceKind === "local");

  const pendingProgress = useMemo(() => {
    if (!selectedOption || selectedOption.id === modelState?.activeId) return null;
    if (progress?.modelId === selectedOption.id) return progress;
    return {
      modelId: selectedOption.id,
      state: selectedOption.state,
      bytesDownloaded: selectedOption.bytesDownloaded,
      bytesTotal: selectedOption.bytesTotal,
    } satisfies ModelInstallProgressPayload;
  }, [modelState?.activeId, progress, selectedOption]);

  const pendingError = pendingProgress ? stateError(pendingProgress.state) : null;
  const pendingIsBusy = Boolean(
    pendingProgress &&
    !pendingError &&
    (BUSY_STATES.has(pendingProgress.state) || selectedOption?.id !== modelState?.activeId),
  );
  const progressPercent = pendingProgress?.bytesTotal
    ? Math.min(
        100,
        Math.max(
          0,
          Math.round((pendingProgress.bytesDownloaded / pendingProgress.bytesTotal) * 100),
        ),
      )
    : null;

  const chooseOption = async (option: ModelOption, retry = false) => {
    if (!retry && option.id === selectedId) return;
    const requestSequence = ++requestSequenceRef.current;
    setRequesting(true);
    setChoosingLocal(false);
    setActionError(null);
    setProgress({
      modelId: option.id,
      state: option.installed ? "preparing" : "queued",
      bytesDownloaded: option.bytesDownloaded,
      bytesTotal: option.bytesTotal,
    });
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

  const detailOption = selectedOption ?? activeOption ?? recommendedOption;
  const activeName = modelName(activeOption);

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
        <CardHeader className="border-b">
          <CardTitle>AI model</CardTitle>
          <CardDescription>
            Used for every conversation, goal, and background task in Doce.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0">
              <p id="model-selector-label" className="font-medium">
                Model
              </p>
              <p id="model-selector-description" className="text-sm text-muted-foreground">
                Choose one model for everything.
              </p>
            </div>

            <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen} modal={false}>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    className="w-full min-w-48 justify-between sm:w-auto sm:max-w-72"
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
                {requesting ? <Spinner className="ml-1" /> : <ChevronsUpDown className="ml-1" />}
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
                  {curatedOptions.map((option) => (
                    <DropdownMenuRadioItem
                      key={option.id}
                      value={option.id}
                      className="items-start py-2"
                      data-testid={`model-option-${option.id}`}
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
                          {option.installed ? "Ready to use" : "Downloads when selected"}
                        </span>
                      </span>
                    </DropdownMenuRadioItem>
                  ))}

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
                        >
                          <span className="min-w-0 flex-1">
                            <span
                              className="block truncate font-medium"
                              title={option.localPath ?? undefined}
                            >
                              {modelName(option)}
                            </span>
                            <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                              {option.localPath}
                            </span>
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
            className="flex items-start gap-3 rounded-lg border bg-muted/40 p-3"
            data-testid="active-model-summary"
            role="status"
            aria-live="polite"
            aria-atomic="true"
          >
            <span className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-background ring-1 ring-foreground/10">
              {activeOption ? (
                <CheckCircle2 className="size-4 text-primary" />
              ) : (
                <Spinner className="size-4 text-muted-foreground" />
              )}
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-medium">{activeName}</span>
                {activeOption ? <Badge variant="outline">Active</Badge> : null}
              </div>
              <p className="mt-0.5 text-sm text-muted-foreground">
                {activeOption?.description ?? "Doce is preparing a model for first use."}
              </p>
              {activeOption?.sourceKind === "local" && activeOption.localPath ? (
                <p
                  className="mt-1 truncate text-xs text-muted-foreground"
                  title={activeOption.localPath}
                >
                  {activeOption.localPath}
                </p>
              ) : null}
            </div>
          </div>

          {pendingIsBusy && pendingProgress && selectedOption ? (
            <div
              className="rounded-lg border p-3"
              data-testid="model-progress"
              role="status"
              aria-live="polite"
            >
              <Progress value={progressPercent} aria-label="Model preparation progress">
                <ProgressLabel className="flex items-center gap-1.5 text-xs">
                  {progressPercent === null ? <Spinner className="size-3.5" /> : null}
                  {progressCopy(pendingProgress.state, modelName(selectedOption))}
                </ProgressLabel>
                <span className="ml-auto text-xs text-muted-foreground tabular-nums">
                  {progressPercent === null ? "Preparing…" : `${progressPercent}%`}
                </span>
              </Progress>
              <p className="mt-2 text-xs text-muted-foreground">
                {activeOption
                  ? `${activeName} stays active until the new model is ready.`
                  : "Doce will start using this model when it is ready."}
              </p>
            </div>
          ) : null}

          {choosingLocal ? (
            <div
              className="flex items-center gap-2 rounded-lg border p-3 text-sm"
              aria-live="polite"
            >
              <Spinner />
              Checking the selected model…
            </div>
          ) : null}

          {pendingError ? (
            <Alert variant="destructive" data-testid="model-error">
              <AlertTitle>Couldn’t switch models</AlertTitle>
              <AlertDescription>
                {pendingError} {activeOption ? `${activeName} is still active.` : null}
              </AlertDescription>
              <div className="mt-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => selectedOption && void chooseOption(selectedOption, true)}
                  disabled={requesting}
                >
                  <RotateCcw />
                  Retry
                </Button>
              </div>
            </Alert>
          ) : null}

          {actionError ? (
            <Alert variant="destructive" data-testid="model-action-error">
              <AlertTitle>Model unchanged</AlertTitle>
              <AlertDescription>
                {actionError} {activeOption ? `${activeName} is still active.` : null}
              </AlertDescription>
            </Alert>
          ) : null}

          {recommendedOption ? (
            <div
              className="flex items-start gap-2 text-xs text-muted-foreground"
              data-testid="model-recommendation"
            >
              <Sparkles className="mt-px size-3.5 shrink-0 text-primary" />
              <span>
                <strong className="font-medium text-foreground">
                  {modelName(recommendedOption)}
                </strong>{" "}
                {modelState?.hardware.ramGb
                  ? " is recommended for this Mac."
                  : " works well on most Macs."}
              </span>
            </div>
          ) : null}

          <div className="flex flex-col gap-2 border-t pt-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <p className="font-medium">Model from this Mac</p>
              <p className="text-sm text-muted-foreground">
                Choose a compatible GGUF file. Doce checks it before switching.
              </p>
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => void chooseLocalFile()}
              disabled={choosingLocal}
              data-testid="choose-local-model-button"
            >
              {choosingLocal ? <Spinner /> : <FileUp />}
              Choose…
            </Button>
          </div>
        </CardContent>
      </Card>

      {detailOption ? (
        <details className="group mt-2 text-sm" data-testid="model-technical-details">
          <summary className="flex cursor-pointer list-none items-center gap-1 py-1 text-xs text-muted-foreground hover:text-foreground">
            <ChevronsUpDown className="size-3" />
            Technical details
          </summary>
          <dl className="mt-1 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 rounded-lg border bg-card p-3 text-xs">
            <dt className="text-muted-foreground">Model</dt>
            <dd className="truncate text-right" title={detailOption.technicalName}>
              {detailOption.technicalName}
            </dd>
            {detailOption.sourceKind === "curated" ? (
              <>
                <dt className="text-muted-foreground">Parameters</dt>
                <dd className="text-right">{formatParameters(detailOption.parameterCount)}</dd>
              </>
            ) : null}
            <dt className="text-muted-foreground">Format</dt>
            <dd className="text-right">
              {detailOption.quantization.toUpperCase() === "GGUF"
                ? "GGUF"
                : `GGUF · ${detailOption.quantization}`}
            </dd>
            <dt className="text-muted-foreground">Size</dt>
            <dd className="text-right">{formatBytes(detailOption.sizeBytes)}</dd>
            <dt className="text-muted-foreground">Location</dt>
            <dd className="truncate text-right" title={detailOption.localPath ?? "Managed by Doce"}>
              {detailOption.localPath ?? "Managed by Doce"}
            </dd>
          </dl>
        </details>
      ) : null}
    </section>
  );
}
