import { useMemo, useState } from "react";
import { CheckCircle2, ChevronDown, Server, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { commands, type EndpointTestResult, type ModelState } from "@/lib/ipc";

// "approach E": a segmented Kind control reshapes this form in place. Each kind
// picks sensible defaults (key visibility, prompt caching) and its own copy —
// the behavioral bit the backend cares about is `useCachePrompt`; `kind` itself
// is informational.
export type EndpointKind = "local" | "hosted" | "lan";

interface KindConfig {
  label: string;
  apiKey: "hidden" | "required" | "optional";
  useCachePrompt: boolean;
  urlPlaceholder: string;
  // Rendered with the entered host when present.
  privacy: (host: string | null) => string;
}

const KIND_CONFIG: Record<EndpointKind, KindConfig> = {
  local: {
    label: "Local server",
    apiKey: "hidden",
    useCachePrompt: true,
    urlPlaceholder: "http://localhost:8080/v1",
    privacy: () => "Runs on this Mac.",
  },
  hosted: {
    label: "Hosted API",
    apiKey: "required",
    useCachePrompt: false,
    urlPlaceholder: "https://openrouter.ai/api/v1",
    privacy: (host) => (host ? `Requests go to ${host}.` : "Requests go to the host you enter."),
  },
  lan: {
    label: "LAN cluster",
    apiKey: "optional",
    useCachePrompt: false,
    urlPlaceholder: "http://192.168.1.50:11434/v1",
    privacy: () => "Stays on your network.",
  },
};

const KIND_ORDER: EndpointKind[] = ["local", "hosted", "lan"];
const DEFAULT_CONTEXT_WINDOW = 32768;

export interface EndpointPrefill {
  kind: EndpointKind;
  url: string;
  model: string;
}

/** The host of a base URL, or null when it isn't yet a parseable URL — used
 * both for the live privacy note and to seed a sensible kind when re-opening
 * an existing endpoint (whose kind the backend doesn't persist). */
export function hostFromUrl(url: string): string | null {
  try {
    return new URL(url.trim()).host || null;
  } catch {
    return null;
  }
}

/** Infers a starting kind from a saved endpoint URL: loopback → local, a
 * private LAN address → lan, anything else → hosted. */
export function inferEndpointKind(url: string): EndpointKind {
  const host = hostFromUrl(url);
  if (!host) return "hosted";
  const name = host.replace(/:\d+$/, "").toLowerCase();
  if (name === "localhost" || name === "127.0.0.1" || name === "::1" || name === "[::1]") {
    return "local";
  }
  if (
    name.startsWith("10.") ||
    name.startsWith("192.168.") ||
    /^172\.(1[6-9]|2\d|3[01])\./.test(name) ||
    name.endsWith(".local")
  ) {
    return "lan";
  }
  return "hosted";
}

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) return error.message;
  return typeof error === "string" && error ? error : fallback;
}

export interface AddModelEndpointProps {
  /** Set when re-opening an existing endpoint: the form pre-fills the URL and
   * model. The saved API key is never echoed back, so the user re-confirms it. */
  prefill?: EndpointPrefill | null;
  onCancel: () => void;
  onSaved: (state: ModelState) => void;
}

export default function AddModelEndpoint({ prefill, onCancel, onSaved }: AddModelEndpointProps) {
  const isReopen = Boolean(prefill);
  const [kind, setKind] = useState<EndpointKind>(prefill?.kind ?? "local");
  const [url, setUrl] = useState(prefill?.url ?? "");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState(prefill?.model ?? "");
  const [contextWindow, setContextWindow] = useState(String(DEFAULT_CONTEXT_WINDOW));
  const [useCachePrompt, setUseCachePrompt] = useState(
    KIND_CONFIG[prefill?.kind ?? "local"].useCachePrompt,
  );
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<EndpointTestResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const config = KIND_CONFIG[kind];
  const host = useMemo(() => hostFromUrl(url), [url]);
  const showApiKey = config.apiKey !== "hidden";
  // The model list is revealed only once a Test succeeds. `ok` with no models
  // (a reachable endpoint that won't enumerate) falls back to free text.
  const connected = testResult?.ok === true;
  const models = testResult?.models ?? [];

  const changeKind = (next: EndpointKind) => {
    setKind(next);
    // A kind switch resets the caching default and drops any prior probe — the
    // model list belongs to the URL/kind you just left.
    setUseCachePrompt(KIND_CONFIG[next].useCachePrompt);
    setTestResult(null);
    setTestError(null);
    setSaveError(null);
  };

  const changeUrl = (next: string) => {
    setUrl(next);
    // The revealed model list is only valid for the URL it was fetched from.
    setTestResult(null);
    setTestError(null);
  };

  const trimmedUrl = url.trim();
  const trimmedKey = apiKey.trim();
  const keyForCall = showApiKey && trimmedKey ? trimmedKey : undefined;

  const test = async () => {
    if (!trimmedUrl) return;
    setTesting(true);
    setTestError(null);
    setSaveError(null);
    try {
      const result = await commands.testModelEndpoint(trimmedUrl, keyForCall);
      setTestResult(result);
      if (!result.ok) {
        setTestError(result.error ?? "Doce couldn’t reach this endpoint.");
      } else if (result.models.length > 0 && !result.models.includes(model)) {
        // Seed the dropdown with the first offered model unless the pre-filled
        // one is still on offer.
        setModel(result.models[0]);
      }
    } catch (error) {
      setTestResult(null);
      setTestError(
        errorMessage(error, "Doce couldn’t reach this endpoint. Check the URL, then Test again."),
      );
    } finally {
      setTesting(false);
    }
  };

  const parsedWindow = Number.parseInt(contextWindow, 10);
  const contextWindowValue = Number.isFinite(parsedWindow) && parsedWindow > 0 ? parsedWindow : 0;
  const keyMissing = config.apiKey === "required" && !trimmedKey;
  const canSave = Boolean(trimmedUrl) && Boolean(model.trim()) && !keyMissing && !saving;

  const save = async () => {
    if (!canSave) return;
    setSaving(true);
    setSaveError(null);
    try {
      const next = await commands.selectEndpointModel({
        kind,
        url: trimmedUrl,
        model: model.trim(),
        apiKey: keyForCall ?? null,
        contextWindow: contextWindowValue,
        useCachePrompt,
      });
      onSaved(next);
    } catch (error) {
      setSaveError(
        errorMessage(error, "Doce couldn’t save this endpoint. Check the fields, then try again."),
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card size="sm" data-testid="add-endpoint-form">
      <CardContent className="space-y-4">
        <div className="flex items-start gap-2.5">
          <Server className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0">
            <p className="font-medium">
              {isReopen ? "Reconnect model endpoint" : "Add a model endpoint"}
            </p>
            <p className="text-sm text-muted-foreground">
              Point Doce at an OpenAI-compatible server. Pick where it runs, then Test to load its
              models.
            </p>
          </div>
        </div>

        <Field className="gap-1.5">
          <FieldLabel className="text-xs text-muted-foreground">Kind</FieldLabel>
          <ToggleGroup
            value={[kind]}
            onValueChange={(values) => {
              const next = (values[0] as EndpointKind) ?? kind;
              if (next !== kind) changeKind(next);
            }}
            spacing={0}
            variant="outline"
            className="w-full"
            data-testid="endpoint-kind"
          >
            {KIND_ORDER.map((value) => (
              <ToggleGroupItem
                key={value}
                value={value}
                variant="outline"
                className="flex-1"
                data-testid={`endpoint-kind-${value}`}
              >
                {KIND_CONFIG[value].label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Field>

        <Field className="gap-1">
          <FieldLabel htmlFor="endpoint-url" className="text-xs text-muted-foreground">
            Base URL
          </FieldLabel>
          <Input
            id="endpoint-url"
            data-testid="endpoint-url-input"
            placeholder={config.urlPlaceholder}
            value={url}
            onChange={(event) => changeUrl(event.target.value)}
          />
        </Field>

        {showApiKey ? (
          <Field className="gap-1">
            <FieldLabel htmlFor="endpoint-api-key" className="text-xs text-muted-foreground">
              API key{config.apiKey === "optional" ? " (optional)" : ""}
            </FieldLabel>
            <Input
              id="endpoint-api-key"
              data-testid="endpoint-api-key-input"
              type="password"
              placeholder="sk-…"
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
            />
            <FieldDescription>
              Stored in your macOS Keychain, never in the app.
              {isReopen ? " The saved key isn’t shown — re-enter it to keep access." : ""}
            </FieldDescription>
          </Field>
        ) : null}

        <div
          className="flex items-start gap-2.5 rounded-lg border border-border bg-muted/40 p-3"
          data-testid="endpoint-privacy-note"
        >
          <ShieldCheck size={16} className="mt-0.5 shrink-0 text-muted-foreground" />
          <p className="text-xs text-muted-foreground">{config.privacy(host)}</p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void test()}
            disabled={!trimmedUrl || testing}
            data-testid="endpoint-test-button"
          >
            {testing ? <Spinner /> : null}
            {testing ? "Testing…" : "Test"}
          </Button>
          {connected ? (
            <span
              className="flex items-center gap-1.5 text-xs text-muted-foreground"
              data-testid="endpoint-test-status"
            >
              <CheckCircle2 className="size-3.5 text-primary" />
              Connected — {models.length} {models.length === 1 ? "model" : "models"}
            </span>
          ) : null}
        </div>

        {testError ? (
          <p className="text-sm text-destructive" data-testid="endpoint-test-error">
            {testError}
          </p>
        ) : null}

        {connected ? (
          <Field className="gap-1" data-testid="endpoint-model-field">
            <FieldLabel htmlFor="endpoint-model" className="text-xs text-muted-foreground">
              Model
            </FieldLabel>
            {models.length > 0 ? (
              <NativeSelect
                id="endpoint-model"
                data-testid="endpoint-model-select"
                className="w-full"
                value={model}
                onChange={(event) => setModel(event.target.value)}
              >
                {models.map((name) => (
                  <NativeSelectOption key={name} value={name}>
                    {name}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            ) : (
              <>
                <Input
                  id="endpoint-model"
                  data-testid="endpoint-model-input"
                  placeholder="model-id"
                  value={model}
                  onChange={(event) => setModel(event.target.value)}
                />
                <FieldDescription>
                  This endpoint didn’t list its models — type the model id to use.
                </FieldDescription>
              </>
            )}
          </Field>
        ) : null}

        <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
          <CollapsibleTrigger
            render={
              <Button
                type="button"
                variant="ghost"
                size="xs"
                className="text-muted-foreground"
                data-testid="endpoint-advanced-toggle"
              />
            }
          >
            <ChevronDown
              className={advancedOpen ? "rotate-180 transition-transform" : "transition-transform"}
            />
            Advanced
          </CollapsibleTrigger>
          <CollapsibleContent className="space-y-4 pt-3">
            <Field className="gap-1">
              <FieldLabel
                htmlFor="endpoint-context-window"
                className="text-xs text-muted-foreground"
              >
                Context window
              </FieldLabel>
              <Input
                id="endpoint-context-window"
                data-testid="endpoint-context-window-input"
                type="number"
                min={0}
                className="w-40"
                value={contextWindow}
                onChange={(event) => setContextWindow(event.target.value)}
              />
              <FieldDescription>
                Tokens the model keeps in context. Leave 0 for the built-in default.
              </FieldDescription>
            </Field>

            {kind !== "hosted" ? (
              <Field orientation="horizontal" className="items-center justify-between">
                <FieldLabel htmlFor="endpoint-cache-prompt" className="gap-0.5">
                  <span className="text-sm font-medium">llama.cpp server</span>
                  <FieldDescription>
                    Enable prompt caching for a local llama.cpp server.
                  </FieldDescription>
                </FieldLabel>
                <Switch
                  id="endpoint-cache-prompt"
                  data-testid="endpoint-cache-prompt-switch"
                  checked={useCachePrompt}
                  onCheckedChange={(value) => setUseCachePrompt(value === true)}
                />
              </Field>
            ) : null}
          </CollapsibleContent>
        </Collapsible>

        {saveError ? (
          <p className="text-sm text-destructive" data-testid="endpoint-save-error">
            {saveError}
          </p>
        ) : null}

        <div className="flex items-center gap-2 border-t pt-4">
          <Button
            type="button"
            onClick={() => void save()}
            disabled={!canSave}
            data-testid="endpoint-save-button"
          >
            {saving ? <Spinner /> : null}
            {saving ? "Saving…" : isReopen ? "Reconnect" : "Add endpoint"}
          </Button>
          <Button
            type="button"
            variant="ghost"
            onClick={onCancel}
            disabled={saving}
            data-testid="endpoint-cancel-button"
          >
            Cancel
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
