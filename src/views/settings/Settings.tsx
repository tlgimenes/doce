import { useCallback, useEffect, useState } from "react";
import { useTheme } from "next-themes";
import { X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
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
import { commands, type McpServerConnection, type SkillSummary } from "@/lib/ipc";
import Connections from "./Connections";
import ModelSelector from "./ModelSelector";

interface SettingsProps {
  onClose: () => void;
}

const THEME_LABELS: Record<string, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};

export default function Settings({ onClose }: SettingsProps) {
  const { theme, setTheme } = useTheme();
  const [servers, setServers] = useState<McpServerConnection[]>([]);
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [addError, setAddError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsInput, setArgsInput] = useState("");
  const [toolsByServer, setToolsByServer] = useState<Record<string, string[] | "error">>({});

  const refresh = useCallback(() => {
    void commands
      .listMcpServers()
      .then(setServers)
      .catch(() => {
        // Model and Skills settings remain independently usable if MCP
        // discovery is temporarily unavailable.
      });
    void commands
      .listSkills()
      .then(setSkills)
      .catch(() => {
        // Keep the rest of the consolidated screen mounted on discovery
        // failures; a future refresh can reconcile the list.
      });
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

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
    } catch (error) {
      setAddError(error instanceof Error ? error.message : String(error));
    }
  };

  const testServer = async (serverId: string) => {
    try {
      const tools = await commands.listMcpServerTools(serverId);
      setToolsByServer((previous) => ({
        ...previous,
        [serverId]: tools.map((tool) => tool.name),
      }));
    } catch {
      setToolsByServer((previous) => ({ ...previous, [serverId]: "error" }));
    }
  };

  return (
    <div
      className="flex h-full flex-col overflow-y-auto bg-background p-6 text-foreground"
      data-testid="settings-view"
    >
      <div className="mx-auto w-full max-w-2xl">
        <div className="mb-6 flex items-center justify-between">
          <div>
            <h2 className="text-balance text-lg font-medium">Settings</h2>
            <p className="mt-0.5 text-sm text-muted-foreground">
              Manage how Doce looks, works, and connects on this Mac.
            </p>
          </div>
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

        <section
          className="mb-8"
          aria-labelledby="general-settings-heading"
          data-testid="settings-general-section"
        >
          <h3
            id="general-settings-heading"
            className="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase"
          >
            General
          </h3>
          <Card size="sm" data-testid="settings-appearance-panel">
            <CardContent>
              <Field orientation="horizontal" className="items-center justify-between gap-4">
                <FieldContent>
                  <FieldLabel htmlFor="theme-select">Appearance</FieldLabel>
                  <FieldDescription>Match this Mac, or always use light or dark.</FieldDescription>
                </FieldContent>
                <Select
                  items={THEME_LABELS}
                  value={theme ?? "system"}
                  onValueChange={(value) => setTheme(value ?? "system")}
                >
                  <SelectTrigger
                    id="theme-select"
                    data-testid="theme-select"
                    aria-label="Appearance"
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
        </section>

        <ModelSelector />

        <section
          aria-labelledby="extensions-settings-heading"
          data-testid="settings-extensions-section"
        >
          <h3
            id="extensions-settings-heading"
            className="mb-3 text-xs font-medium tracking-wide text-muted-foreground uppercase"
          >
            Extensions
          </h3>

          <div className="space-y-8">
            <div data-testid="settings-connections-panel">
              <Connections />
            </div>

            <section aria-labelledby="mcp-settings-heading" data-testid="settings-mcp-panel">
              <div className="mb-3">
                <h4 id="mcp-settings-heading" className="text-sm font-medium">
                  MCP servers
                </h4>
                <p className="mt-0.5 text-sm text-muted-foreground">
                  Connect Doce to tools and business services.
                </p>
              </div>

              <Card size="sm" className="mb-4">
                <CardContent>
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
                        onChange={(event) => setName(event.target.value)}
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
                        onChange={(event) => setCommand(event.target.value)}
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
                        onChange={(event) => setArgsInput(event.target.value)}
                        data-testid="mcp-args-input"
                      />
                    </Field>
                    <Button
                      variant="default"
                      onClick={addServer}
                      disabled={!name.trim() || !command.trim()}
                      data-testid="add-mcp-server"
                    >
                      Add
                    </Button>
                  </div>
                  {addError ? (
                    <p className="mt-2 text-sm text-destructive" data-testid="mcp-add-error">
                      {addError}
                    </p>
                  ) : null}
                </CardContent>
              </Card>

              <ItemGroup className="gap-3">
                {servers.map((server) => (
                  <Item
                    key={server.id}
                    variant="outline"
                    className="bg-card text-sm"
                    data-testid="mcp-server-item"
                  >
                    <ItemContent className="min-w-0 gap-2">
                      <ItemTitle className="flex-wrap">
                        <span>{server.name}</span>
                        <span className="flex flex-wrap items-center gap-2">
                          <Badge variant="outline">{server.transport}</Badge>
                          <Badge variant={server.enabled ? "default" : "secondary"}>
                            {server.enabled ? "Enabled" : "Disabled"}
                          </Badge>
                        </span>
                      </ItemTitle>
                      {toolsByServer[server.id] === "error" ? (
                        <ItemDescription className="text-xs text-destructive">
                          Failed to connect
                        </ItemDescription>
                      ) : null}
                      {Array.isArray(toolsByServer[server.id]) ? (
                        <ItemDescription
                          className="text-xs text-muted-foreground"
                          data-testid="mcp-server-tools"
                        >
                          Tools: {(toolsByServer[server.id] as string[]).join(", ") || "(none)"}
                        </ItemDescription>
                      ) : null}
                    </ItemContent>
                    <ItemActions className="ml-auto self-start">
                      <Button
                        variant="link"
                        size="sm"
                        onClick={() => void testServer(server.id)}
                        data-testid="test-mcp-server"
                      >
                        Test connection
                      </Button>
                    </ItemActions>
                  </Item>
                ))}
              </ItemGroup>
            </section>

            <section aria-labelledby="skills-settings-heading" data-testid="settings-skills-panel">
              <div className="mb-3">
                <h4 id="skills-settings-heading" className="text-sm font-medium">
                  Skills
                </h4>
                <p className="mt-0.5 text-sm text-muted-foreground">
                  Use specialized workflows available on this Mac.
                </p>
              </div>
              {skills.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  No skills found. Add a folder with a SKILL.md to your skills directory.
                </p>
              ) : (
                <ItemGroup className="gap-2">
                  {skills.map((skill) => (
                    <Item
                      key={skill.name}
                      variant="outline"
                      className="bg-card text-sm"
                      data-testid="skill-item"
                    >
                      <ItemContent>
                        <ItemTitle>{skill.name}</ItemTitle>
                        <ItemDescription>{skill.description}</ItemDescription>
                      </ItemContent>
                    </Item>
                  ))}
                </ItemGroup>
              )}
            </section>
          </div>
        </section>

        {/* Which build is running — the package version plus the exact build
            commit (dev or a released .dmg), injected at build time. */}
        <p
          className="mt-8 text-center text-xs text-muted-foreground"
          data-testid="settings-version"
        >
          doce v{__APP_VERSION__}
          {__GIT_COMMIT__ !== "unknown" && (
            <>
              {" · "}
              <span className="font-mono">{__GIT_COMMIT__}</span>
            </>
          )}
        </p>
      </div>
    </div>
  );
}
