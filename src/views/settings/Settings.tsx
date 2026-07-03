import { useEffect, useState } from "react";
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
    await commands.addMcpServer(name.trim(), command.trim(), args);
    setName("");
    setCommand("");
    setArgsInput("");
    refresh();
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
    <div className="flex h-dvh flex-col overflow-y-auto bg-background p-6 text-foreground" data-testid="settings-view">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-balance text-lg font-medium">Settings</h2>
        <button className="text-sm text-muted-foreground underline" onClick={onClose} data-testid="close-settings">
          Close
        </button>
      </div>

      <section className="mb-8">
        <h3 className="mb-2 text-sm font-medium">MCP servers</h3>
        <div className="mb-3 flex gap-2">
          <input
            className="rounded-md border border-border bg-card px-2 py-1 text-sm"
            placeholder="name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            data-testid="mcp-name-input"
          />
          <input
            className="rounded-md border border-border bg-card px-2 py-1 text-sm"
            placeholder="command (e.g. npx)"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            data-testid="mcp-command-input"
          />
          <input
            className="flex-1 rounded-md border border-border bg-card px-2 py-1 text-sm"
            placeholder="args (space-separated)"
            value={argsInput}
            onChange={(e) => setArgsInput(e.target.value)}
            data-testid="mcp-args-input"
          />
          <button
            className="rounded-md bg-primary px-3 py-1 text-sm text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
            onClick={addServer}
            disabled={!name.trim() || !command.trim()}
            data-testid="add-mcp-server"
          >
            Add
          </button>
        </div>
        <ul className="space-y-2">
          {servers.map((s) => (
            <li key={s.id} className="rounded-md bg-card p-2 text-sm" data-testid="mcp-server-item">
              <div className="flex items-center justify-between">
                <span>
                  {s.name} <span className="text-muted-foreground">({s.transport})</span>
                </span>
                <button className="text-xs underline" onClick={() => testServer(s.id)} data-testid="test-mcp-server">
                  Test connection
                </button>
              </div>
              {toolsByServer[s.id] === "error" && (
                <p className="mt-1 text-xs text-destructive">Failed to connect</p>
              )}
              {Array.isArray(toolsByServer[s.id]) && (
                <p className="mt-1 text-xs text-muted-foreground" data-testid="mcp-server-tools">
                  Tools: {(toolsByServer[s.id] as string[]).join(", ") || "(none)"}
                </p>
              )}
            </li>
          ))}
        </ul>
      </section>

      <section>
        <h3 className="mb-2 text-sm font-medium">Skills</h3>
        {skills.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No skills found. Add a folder with a SKILL.md to your skills directory.
          </p>
        ) : (
          <ul className="space-y-2">
            {skills.map((s) => (
              <li key={s.name} className="rounded-md bg-card p-2 text-sm" data-testid="skill-item">
                <span className="font-medium">{s.name}</span>
                <span className="ml-2 text-muted-foreground">{s.description}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
