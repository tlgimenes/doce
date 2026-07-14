import { useEffect, useState } from "react";
import { useTheme } from "next-themes";
import { X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldContent, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemTitle,
} from "@/components/ui/item";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  commands,
  events,
  type AvailableModel,
  type McpServerConnection,
  type SkillSummary,
} from "@/lib/ipc";

interface SettingsProps {
  onClose: () => void;
}

// Maps next-themes' raw theme values to display labels — without an
// `items` map, Base UI's <Select.Value> renders the raw selected value
// verbatim (e.g. "system"), not the matching <SelectItem>'s children.
const THEME_LABELS: Record<string, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};

/**
 * User Story 4: MCP server registration (FR-018/FR-019) and filesystem
 * skill discovery (FR-020). Minimal by design — connection testing lists
 * a server's tools on demand rather than keeping a live session, and
 * skills are read-only here (added by dropping a `SKILL.md` folder into
 * `<app data dir>/skills`, not managed through this UI yet).
 */
export default function Settings({ onClose }: SettingsProps) {
  const { theme, setTheme } = useTheme();
  const [servers, setServers] = useState<McpServerConnection[]>([]);
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [activeTab, setActiveTab] = useState<"mcp" | "skills">("mcp");
  const [addError, setAddError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsInput, setArgsInput] = useState("");
  const [toolsByServer, setToolsByServer] = useState<Record<string, string[] | "error">>({});
  const [models, setModels] = useState<AvailableModel[]>([]);
  // modelId -> download percent (0-100) while an install runs.
  const [installProgress, setInstallProgress] = useState<Record<string, number>>({});

  const refresh = () => {
    commands.listMcpServers().then(setServers);
    commands.listSkills().then(setSkills);
    commands
      .listAvailableModels()
      .then(setModels)
      .catch(() => {});
  };

  useEffect(() => {
    refresh();
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void events
      .onModelInstallProgress((p) => {
        if (cancelled) return;
        if (p.state === "installed" || p.state.startsWith("error")) {
          setInstallProgress((prev) => {
            const next = { ...prev };
            delete next[p.modelId];
            return next;
          });
          commands
            .listAvailableModels()
            .then((next) => {
              if (!cancelled) setModels(next);
            })
            .catch(() => {});
          return;
        }
        if (p.bytesTotal > 0) {
          setInstallProgress((prev) => ({
            ...prev,
            [p.modelId]: Math.round((p.bytesDownloaded / p.bytesTotal) * 100),
          }));
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const installModel = async (modelId: string) => {
    setInstallProgress((prev) => ({ ...prev, [modelId]: 0 }));
    try {
      await commands.startModelInstall(modelId);
    } catch {
      setInstallProgress((prev) => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
    }
  };

  const activateModel = async (modelId: string) => {
    await commands.setActiveModel(modelId);
    commands
      .listAvailableModels()
      .then(setModels)
      .catch(() => {});
  };

  const addServer = async () => {
    if (!name.trim() || !command.trim()) return;
    const args = argsInput.trim() ? argsInput.trim().split(/\s+/) : [];
    setAddError(null);
    try {
      await commands.addMcpServer(name.trim(), command.trim(), args);
      setName("");
      setCommand("");
      setArgsInput("");
      refresh();
    } catch (err) {
      setAddError(err instanceof Error ? err.message : String(err));
    }
  };

  const testServer = async (serverId: string) => {
    try {
      const tools = await commands.listMcpServerTools(serverId);
      setToolsByServer((prev) => ({ ...prev, [serverId]: tools.map((t) => t.name) }));
    } catch {
      setToolsByServer((prev) => ({ ...prev, [serverId]: "error" }));
    }
  };

  return (
    <div
      className="flex h-full flex-col overflow-y-auto bg-background p-6 text-foreground"
      data-testid="settings-view"
    >
      {/* Readable column, not window-wide sprawl — mirrors the chat surface. */}
      <div className="mx-auto w-full max-w-2xl">
        <div className="mb-6 flex items-center justify-between">
          <h2 className="text-balance text-lg font-medium">Settings</h2>
          <Button
            variant="ghost"
            size="icon-xs"
            className="text-muted-foreground hover:bg-accent"
            onClick={onClose}
            aria-label="Close settings"
            data-testid="close-settings"
          >
            <X size={16} />
          </Button>
        </div>

        <Card size="sm" className="mb-6" data-testid="settings-appearance-panel">
          <CardHeader>
            <CardTitle>Appearance</CardTitle>
          </CardHeader>
          <CardContent>
            <Field orientation="horizontal" className="items-center justify-between gap-4">
              <FieldContent>
                <FieldLabel htmlFor="theme-select">Theme</FieldLabel>
                <FieldDescription>Match your system, or force light or dark.</FieldDescription>
              </FieldContent>
              <Select
                items={THEME_LABELS}
                value={theme ?? "system"}
                onValueChange={(value) => setTheme(value ?? "system")}
              >
                <SelectTrigger
                  id="theme-select"
                  data-testid="theme-select"
                  aria-label="Theme"
                  size="sm"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="system">System</SelectItem>
                  <SelectItem value="light">Light</SelectItem>
                  <SelectItem value="dark">Dark</SelectItem>
                </SelectContent>
              </Select>
            </Field>
          </CardContent>
        </Card>

        <Card size="sm" className="mb-6" data-testid="settings-model-panel">
          <CardHeader>
            <CardTitle>Model</CardTitle>
          </CardHeader>
          <CardContent>
            <ItemGroup className="gap-2">
              {models.map((m) => {
                const progress = installProgress[m.modelId];
                const installing = progress != null;
                return (
                  <Item key={m.modelId} size="xs" data-testid="model-item">
                    <ItemContent className="min-w-0 gap-0.5">
                      <ItemTitle className="flex-wrap">
                        <span className="font-mono text-xs">{m.modelId}</span>
                        {m.active && <Badge variant="default">Active</Badge>}
                        {!m.active && m.installed && <Badge variant="outline">Installed</Badge>}
                        {m.recommended && <Badge variant="secondary">Recommended</Badge>}
                      </ItemTitle>
                      <ItemDescription className="text-xs">
                        {[m.quantization, ...m.capabilityTags].join(" · ")}
                      </ItemDescription>
                    </ItemContent>
                    <ItemActions>
                      {installing ? (
                        <span
                          className="font-mono text-xs text-muted-foreground tabular-nums"
                          data-testid="model-install-progress"
                        >
                          {progress}%
                        </span>
                      ) : m.active ? null : m.installed ? (
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => activateModel(m.modelId)}
                          data-testid="activate-model"
                        >
                          Activate
                        </Button>
                      ) : (
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => installModel(m.modelId)}
                          data-testid="install-model"
                        >
                          Install
                        </Button>
                      )}
                    </ItemActions>
                  </Item>
                );
              })}
            </ItemGroup>
          </CardContent>
        </Card>

        <Tabs
          value={activeTab}
          onValueChange={(value) => setActiveTab(value as "mcp" | "skills")}
          className="gap-0"
        >
          <TabsList className="mb-6 rounded-md border border-border bg-card p-1">
            <TabsTrigger
              value="mcp"
              aria-selected={activeTab === "mcp"}
              data-testid="settings-tab-mcp"
              className="min-w-28 rounded-sm px-3 py-1 text-sm data-active:bg-primary data-active:text-primary-foreground"
            >
              MCP Servers
            </TabsTrigger>
            <TabsTrigger
              value="skills"
              aria-selected={activeTab === "skills"}
              data-testid="settings-tab-skills"
              className="min-w-20 rounded-sm px-3 py-1 text-sm data-active:bg-primary data-active:text-primary-foreground"
            >
              Skills
            </TabsTrigger>
          </TabsList>

          <TabsContent value="mcp">
            {activeTab === "mcp" && (
              <section data-testid="settings-mcp-panel">
                <Card size="sm" className="mb-4">
                  <CardHeader>
                    <CardTitle>MCP servers</CardTitle>
                  </CardHeader>
                  <CardContent>
                    {/* items-end bottom-aligns the Add button with the input
                    row — the labels above the inputs otherwise leave it
                    floating. */}
                    <div className="flex flex-wrap items-end gap-2">
                      <Field className="min-w-40 flex-1 gap-1">
                        <FieldLabel
                          htmlFor="mcp-name-input"
                          className="text-xs text-muted-foreground"
                        >
                          Server name
                        </FieldLabel>
                        <Input
                          id="mcp-name-input"
                          placeholder="name"
                          value={name}
                          onChange={(e) => setName(e.target.value)}
                          data-testid="mcp-name-input"
                        />
                      </Field>
                      <Field className="min-w-48 flex-1 gap-1">
                        <FieldLabel
                          htmlFor="mcp-command-input"
                          className="text-xs text-muted-foreground"
                        >
                          Command
                        </FieldLabel>
                        <Input
                          id="mcp-command-input"
                          placeholder="command (e.g. npx)"
                          value={command}
                          onChange={(e) => setCommand(e.target.value)}
                          data-testid="mcp-command-input"
                        />
                      </Field>
                      <Field className="min-w-56 flex-[2] gap-1">
                        <FieldLabel
                          htmlFor="mcp-args-input"
                          className="text-xs text-muted-foreground"
                        >
                          Arguments
                        </FieldLabel>
                        <Input
                          id="mcp-args-input"
                          placeholder="args (space-separated)"
                          value={argsInput}
                          onChange={(e) => setArgsInput(e.target.value)}
                          data-testid="mcp-args-input"
                        />
                      </Field>
                      <Button
                        variant="default"
                        size="sm"
                        onClick={addServer}
                        disabled={!name.trim() || !command.trim()}
                        data-testid="add-mcp-server"
                      >
                        Add
                      </Button>
                    </div>
                    {addError && (
                      <p className="mt-2 text-sm text-destructive" data-testid="mcp-add-error">
                        {addError}
                      </p>
                    )}
                  </CardContent>
                </Card>

                <ItemGroup className="gap-3">
                  {servers.map((s) => (
                    <Item
                      key={s.id}
                      variant="outline"
                      className="bg-card text-sm"
                      data-testid="mcp-server-item"
                    >
                      <ItemContent className="min-w-0 gap-2">
                        <ItemTitle className="flex-wrap">
                          <span>{s.name}</span>
                          <span className="flex flex-wrap items-center gap-2">
                            <Badge variant="outline">{s.transport}</Badge>
                            <Badge variant={s.enabled ? "default" : "secondary"}>
                              {s.enabled ? "Enabled" : "Disabled"}
                            </Badge>
                          </span>
                        </ItemTitle>
                        {toolsByServer[s.id] === "error" ? (
                          <ItemDescription className="text-xs text-destructive">
                            Failed to connect
                          </ItemDescription>
                        ) : null}
                        {Array.isArray(toolsByServer[s.id]) ? (
                          <ItemDescription
                            className="text-xs text-muted-foreground"
                            data-testid="mcp-server-tools"
                          >
                            Tools: {(toolsByServer[s.id] as string[]).join(", ") || "(none)"}
                          </ItemDescription>
                        ) : null}
                      </ItemContent>
                      <ItemActions className="ml-auto self-start">
                        <Button
                          variant="link"
                          size="sm"
                          onClick={() => testServer(s.id)}
                          data-testid="test-mcp-server"
                        >
                          Test connection
                        </Button>
                      </ItemActions>
                    </Item>
                  ))}
                </ItemGroup>
              </section>
            )}
          </TabsContent>

          <TabsContent value="skills">
            {activeTab === "skills" && (
              <section data-testid="settings-skills-panel">
                <h3 className="mb-3 text-sm font-medium">Skills</h3>
                {skills.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    No skills found. Add a folder with a SKILL.md to your skills directory.
                  </p>
                ) : (
                  <ItemGroup className="gap-2">
                    {skills.map((s) => (
                      <Item
                        key={s.name}
                        variant="outline"
                        className="bg-card text-sm"
                        data-testid="skill-item"
                      >
                        <ItemContent>
                          <ItemTitle>{s.name}</ItemTitle>
                          <ItemDescription>{s.description}</ItemDescription>
                        </ItemContent>
                      </Item>
                    ))}
                  </ItemGroup>
                )}
              </section>
            )}
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
