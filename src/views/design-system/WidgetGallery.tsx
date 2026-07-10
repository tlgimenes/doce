import type { ReactNode } from "react";
import { Command, Folder, Settings2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import ReadWidget from "@/views/chat/tool-widgets/ReadWidget";
import WriteWidget from "@/views/chat/tool-widgets/WriteWidget";
import EditDiffWidget from "@/views/chat/tool-widgets/EditDiffWidget";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import SearchResultsWidget from "@/views/chat/tool-widgets/SearchResultsWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
import AskUserQuestionWidget from "@/views/chat/tool-widgets/AskUserQuestionWidget";
import UserAskWidget from "@/views/chat/tool-widgets/UserAskWidget";
import UnknownToolWidget from "@/views/chat/tool-widgets/UnknownToolWidget";

interface WidgetGalleryProps {
  onClose: () => void;
}

function Example({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <p className="font-mono text-[11px] uppercase tracking-wide text-muted-foreground">{label}</p>
      {children}
    </div>
  );
}

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3">
      <div>
        <h2 className="text-sm font-semibold">{title}</h2>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">{children}</div>
    </section>
  );
}

function Swatch({ name, variable }: { name: string; variable: string }) {
  return (
    <div className="rounded-md border border-border bg-card p-3">
      <div
        className="mb-2 h-12 rounded-sm border border-border"
        style={{ backgroundColor: `var(${variable})` }}
      />
      <div className="space-y-0.5">
        <p className="text-sm font-medium">{name}</p>
        <p className="font-mono text-[11px] text-muted-foreground">{variable}</p>
      </div>
    </div>
  );
}

/**
 * A live catalog of every tool-call widget (the components `MessageContent`
 * dispatches `tool_result` rows to), rendered with hand-built sample data
 * covering each widget's real states -- not a static mockup: these are the
 * exact same widget components/props the real chat transcript uses, so
 * this page can never visually drift from what a user actually sees. Reach
 * via ⌘D (`lib/shortcuts.ts`) -- a reference for iterating on widget
 * styling, not a feature end users need in daily use, so it isn't a
 * permanent sidebar entry the way Settings/Search are.
 */
export default function WidgetGallery({ onClose }: WidgetGalleryProps) {
  return (
    <div
      className="flex h-full flex-col overflow-y-auto bg-background"
      data-testid="widget-gallery"
    >
      <div className="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-background/95 px-6 py-3 backdrop-blur">
        <div>
          <h1 className="text-base font-semibold">Widget gallery</h1>
          <p className="text-xs text-muted-foreground">
            Every tool-call widget, live, across its real states.
          </p>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          onClick={onClose}
          aria-label="Close widget gallery"
          data-testid="widget-gallery-close"
        >
          <X size={16} />
        </Button>
      </div>

      <div className="flex flex-col gap-10 px-6 py-6">
        <Section
          title="Workbench"
          description="Shared app primitives for command access, settings rows, button treatments, and brand accents."
        >
          <Example label="Button variants">
            <div className="flex flex-wrap gap-2">
              <Button variant="primary" size="sm">
                Primary
              </Button>
              <Button variant="secondary" size="sm">
                Secondary
              </Button>
              <Button variant="ghost" size="sm">
                Ghost
              </Button>
              <Button variant="destructive" size="sm">
                Destructive
              </Button>
            </div>
          </Example>
          <Example label="Command center preview">
            <div className="rounded-md border border-border bg-card p-2">
              <div className="px-2 py-2 text-xs font-medium uppercase text-muted-foreground">
                Actions
              </div>
              <div className="space-y-1">
                <div className="flex items-center justify-between rounded-md px-2 py-2 text-sm hover:bg-accent">
                  <span className="inline-flex items-center gap-2">
                    <Command size={14} className="text-muted-foreground" />
                    Open command center
                  </span>
                  <span className="font-mono text-xs text-muted-foreground">Cmd+K</span>
                </div>
                <div className="flex items-center justify-between rounded-md px-2 py-2 text-sm hover:bg-accent">
                  <span className="inline-flex items-center gap-2">
                    <Settings2 size={14} className="text-muted-foreground" />
                    Open settings
                  </span>
                  <span className="font-mono text-xs text-muted-foreground">Cmd+,</span>
                </div>
              </div>
            </div>
          </Example>
          <Example label="Settings row preview">
            <div className="rounded-md border border-border bg-card p-3 text-sm">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <div className="inline-flex items-center gap-2 font-medium">
                    <Folder size={14} className="text-muted-foreground" />
                    Workspace indexing
                  </div>
                  <p className="text-xs text-muted-foreground">
                    Keep recent folders searchable and ready for new conversations.
                  </p>
                </div>
                <Button variant="ghost" size="sm" className="h-auto px-0 text-xs">
                  Test connection
                </Button>
              </div>
            </div>
          </Example>
          <Example label="Brand Accent Workbench">
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
              <Swatch name="Chocolate" variable="--color-doce-chocolate" />
              <Swatch name="Cacao" variable="--color-doce-cacao" />
              <Swatch name="Caramel" variable="--color-doce-caramel" />
              <Swatch name="Peach" variable="--color-doce-peach" />
              <Swatch name="Coral" variable="--color-doce-coral" />
              <Swatch name="Cream" variable="--color-doce-cream" />
            </div>
          </Example>
        </Section>

        <Section
          title="Read"
          description="A collapsed file-reference card with inline expandable preview."
        >
          <Example label="Text read">
            <ReadWidget
              detail={{
                toolName: "Read",
                filePath: "src/agent/dispatch.rs",
                offset: null,
                limit: null,
                payloadRef: "src/agent/dispatch.rs",
                outcome: {
                  ok: true,
                  contentPreview: "pub fn execute(...",
                  contentBytes: 48213,
                  truncated: true,
                },
                tokenCount: 312,
              }}
            />
          </Example>
          <Example label="Text read (legacy row)">
            <ReadWidget
              detail={{
                toolName: "Read",
                filePath: "src/agent/legacy.rs",
                offset: null,
                limit: null,
                outcome: { ok: true, content: "pub fn legacy(...", truncated: false },
                tokenCount: 220,
              }}
            />
          </Example>
          <Example label="Native preview candidate">
            <ReadWidget
              detail={{
                toolName: "Read",
                filePath: "diagram.svg",
                offset: null,
                limit: null,
                outcome: { ok: true, content: "(binary preview candidate)", truncated: false },
                tokenCount: 2048,
              }}
            />
          </Example>
          <Example label="Failure">
            <ReadWidget
              detail={{
                toolName: "Read",
                filePath: "does/not/exist.txt",
                offset: null,
                limit: null,
                outcome: { ok: false, error: "No such file or directory (os error 2)" },
              }}
            />
          </Example>
        </Section>

        <Section title="Write" description="A created/overwritten file card. Success / failure.">
          <Example label="Success">
            <WriteWidget
              detail={{
                toolName: "Write",
                filePath: "src/lib/formatTokenCount.ts",
                contentPreview: "export function formatTokenCount(...",
                byteCount: 842,
                outcome: { ok: true },
              }}
            />
          </Example>
          <Example label="Failure">
            <WriteWidget
              detail={{
                toolName: "Write",
                filePath: "/root/protected.txt",
                contentPreview: "",
                byteCount: 0,
                outcome: { ok: false, error: "Permission denied (os error 13)" },
              }}
            />
          </Example>
        </Section>

        <Section
          title="Edit"
          description="A real, labeled diff computed client-side from oldString/newString. Success / failure."
        >
          <Example label="Success">
            <EditDiffWidget
              detail={{
                toolName: "Edit",
                filePath: "src/agent/plan.rs",
                oldString: "pub const PLANNING_SYSTEM_PROMPT",
                newString:
                  "// Tools: CreatePlan, AddStep, ResumeExecution\npub const PLANNING_SYSTEM_PROMPT",
                replaceAll: false,
                outcome: { ok: true },
              }}
            />
          </Example>
          <Example label="Failure (old_string not found)">
            <EditDiffWidget
              detail={{
                toolName: "Edit",
                filePath: "src/agent/plan.rs",
                oldString: "this text is not in the file",
                newString: "replacement",
                replaceAll: false,
                outcome: { ok: false, error: "old_string not found in file" },
              }}
            />
          </Example>
        </Section>

        <Section
          title="Bash"
          description="Command + status + stdout/stderr, terminal-style. Success / non-zero exit / offloaded / dispatch failure."
        >
          <Example label="Success (exit 0)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "cargo test --lib",
                timeoutMs: null,
                payloadRef: "/tmp/doce/tool-outputs/c1/call-1.txt",
                outcome: {
                  ok: true,
                  exitCode: 0,
                  stdoutPreview: "test result: ok. 202 passed",
                  stdoutBytes: 28,
                  stderrPreview: "",
                  stderrBytes: 0,
                },
                tokenCount: 89,
              }}
            />
          </Example>
          <Example label="Completed, non-zero exit (legacy row)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "cargo build --offline",
                timeoutMs: null,
                outcome: {
                  ok: true,
                  exitCode: 1,
                  stdout: "",
                  stderr: "error[E0063]: missing field `created_at`\n --> src/site_b.rs:8:5",
                },
              }}
            />
          </Example>
          <Example label="Offloaded (large output)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "cargo test --test agent_benchmark tier4_planned -- --ignored --nocapture",
                timeoutMs: null,
                payloadRef: "/tmp/doce/tool-outputs/c1/call-2.txt",
                outcome: {
                  ok: true,
                  exitCode: 0,
                  stdoutPreview: "(truncated preview)",
                  stdoutBytes: 84213,
                  stderrPreview: "",
                  stderrBytes: 0,
                },
              }}
            />
          </Example>
          <Example label="Offloaded (large output, legacy row)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "find / -name '*.log'",
                timeoutMs: null,
                offloadedTo: "/tmp/doce/tool-outputs/c1/call-3.txt",
                outcome: { ok: true, exitCode: 0, stdout: "(truncated preview)", stderr: "" },
              }}
            />
          </Example>
          <Example label="Dispatch failure (denylisted)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "rm -rf ~",
                timeoutMs: null,
                outcome: { ok: false, error: "command rejected: matches a catastrophic pattern" },
              }}
            />
          </Example>
          <Example label="Pending (still running)">
            <BashWidget
              detail={{
                toolName: "Bash",
                command: "cargo test --test agent_benchmark tier4_planned -- --ignored --nocapture",
                timeoutMs: null,
              }}
            />
          </Example>
        </Section>

        <Section
          title="Glob / Grep"
          description="Collapsed search summaries with inline expandable result lists."
        >
          <Example label="Glob, with files">
            <SearchResultsWidget
              detail={{
                toolName: "Glob",
                pattern: "bug_*.txt",
                path: ".",
                matches: ["bug_00.txt", "bug_01.txt", "bug_02.txt"],
                tokenCount: 24,
              }}
            />
          </Example>
          <Example label="Glob, no files">
            <SearchResultsWidget
              detail={{ toolName: "Glob", pattern: "*.nonexistent", path: ".", matches: [] }}
            />
          </Example>
          <Example label="Grep, with matches">
            <SearchResultsWidget
              detail={{
                toolName: "Grep",
                pattern: "// BUG:",
                path: ".",
                glob: null,
                matches: [
                  { path: "bug_00.txt", lineNumber: 1, line: "// BUG: this should compute a + b" },
                  { path: "bug_01.txt", lineNumber: 1, line: "// BUG: this should compute a + b" },
                ],
                tokenCount: 51,
              }}
            />
          </Example>
          <Example label="Grep, no matches">
            <SearchResultsWidget
              detail={{ toolName: "Grep", pattern: "TODO", path: ".", glob: "*.rs", matches: [] }}
            />
          </Example>
        </Section>

        <Section
          title="Task"
          description="A subagent delegation's running/complete status only -- its own tool calls stay isolated on its own conversation row."
        >
          <Example label="Running">
            <TaskWidget
              detail={{
                toolName: "Task",
                prompt: "Investigate why tier4 scores 0/20 and report the root cause",
                subagentConversationId: "design-system-preview",
                state: "running",
              }}
            />
          </Example>
          <Example label="Complete">
            <TaskWidget
              detail={{
                toolName: "Task",
                prompt: "Fix bug_07.txt through bug_19.txt",
                subagentConversationId: "design-system-preview",
                state: "complete",
              }}
            />
          </Example>
        </Section>

        <Section
          title="AskUserQuestion"
          description="An interactive pause/resume prompt, rendered in the composer slot while pending. Picking an option selects it; pressing the send button answers -- single-select, multi-select, and free text all work the same way. Closing it (✕) reveals a free-text fallback. Read-only once answered."
        >
          <Example label="Pending, single-select">
            <UserAskWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-1",
                header: "Ambiguous request",
                question: "Which config file should this apply to?",
                options: [
                  { label: "tauri.conf.json", description: "The app's own config" },
                  { label: "vite.config.ts", description: "The dev server config" },
                ],
                multiSelect: false,
                answer: null,
              }}
            />
          </Example>
          <Example label="Pending, multi-select">
            <UserAskWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-2",
                header: "",
                question: "Which tiers should the rerun cover?",
                options: [
                  { label: "Tier 1", description: "" },
                  { label: "Tier 4", description: "" },
                  { label: "Tier 4 planned", description: "" },
                ],
                multiSelect: true,
                answer: null,
              }}
            />
          </Example>
          <Example label="Pending, free-text fallback">
            <UserAskWidget
              initialMode="text"
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-4",
                header: "",
                question: "Rerun now or wait?",
                options: [
                  { label: "Rerun now", description: "" },
                  { label: "Wait", description: "" },
                ],
                multiSelect: false,
                answer: null,
              }}
            />
          </Example>
          <Example label="Answered">
            <AskUserQuestionWidget
              detail={{
                toolName: "AskUserQuestion",
                questionId: "design-system-preview-3",
                header: "",
                question: "Rerun now or wait?",
                options: [
                  { label: "Rerun now", description: "" },
                  { label: "Wait", description: "" },
                ],
                multiSelect: false,
                answer: ["Rerun now"],
              }}
            />
          </Example>
        </Section>

        <Section
          title="Unknown tool"
          description="The fallback for any toolName without a dedicated widget -- name + a readable dump of its detail payload, never blank."
        >
          <Example label="Unrecognized tool">
            <UnknownToolWidget
              detail={{
                toolName: "WebFetch",
                arguments: { url: "https://example.com" },
                outcome: { ok: false, text: "unknown tool 'WebFetch'" },
              }}
            />
          </Example>
        </Section>
      </div>
    </div>
  );
}
