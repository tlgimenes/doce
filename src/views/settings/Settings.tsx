import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemTitle,
} from "@/components/ui/item";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { commands, type McpServerConnection, type SkillSummary } from "@/lib/ipc";

interface SettingsProps {
  onClose: () => void;
}

/**
 * User Story 4: MCP server registration (FR-018/FR-019) and filesystem
 * skill discovery (FR-020). Minimal by design — connection testing lists
 * a server's tools on demand rather than keeping a live session, and
 * skills are read-only here (added by dropping a `SKILL.md` folder into
 * `<app data dir>/skills`, not managed through this UI yet).
 */
export default function Settings({ onClose }: SettingsProps) {
  const [servers, setServers] = useState<McpServerConnection[]>([]);
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [activeTab, setActiveTab] = useState<"mcp" | "skills">("mcp");
  const [addError, setAddError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsInput, setArgsInput] = useState("");
  const [toolsByServer, setToolsByServer] = useState<Record<string, string[] | "error">>({});

  const refresh = () => {
    commands.listMcpServers().then(setServers);
    commands.listSkills().then(setSkills);
  };

  useEffect(() => {
    refresh();
  }, []);

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
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-balance text-lg font-medium">Settings</h2>
        <Button variant="link" size="sm" onClick={onClose} data-testid="close-settings">
          Close
        </Button>
      </div>

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
              <div className="mb-4 rounded-md border border-border bg-card p-4">
                <h3 className="mb-3 text-sm font-medium">MCP servers</h3>
                <div className="mb-3 flex flex-wrap gap-2">
                  <Field className="min-w-40 flex-1 gap-1">
                    <FieldLabel htmlFor="mcp-name-input" className="text-xs text-muted-foreground">
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
                    <FieldLabel htmlFor="mcp-args-input" className="text-xs text-muted-foreground">
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
                    size="default"
                    className="self-start"
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
              </div>

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
  );
}
