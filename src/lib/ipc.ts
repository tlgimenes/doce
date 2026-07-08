import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Hand-written typed wrappers matching src-tauri/src/commands/*.rs.
// tauri-specta auto-generates a real bindings.ts on first debug run
// (src-tauri/src/lib.rs); this file is the pre-first-run bootstrap and a
// fallback for the commands specta doesn't cover 1:1 yet.

export interface HardwareProfile {
  tier: string;
  ramGb: number;
  chip: string;
  diskFreeGb: number;
}

export interface StartModelInstallResult {
  modelId: string;
  resumed: boolean;
}

export interface ModelRow {
  id: string;
  hardwareTier: string;
  isActive: boolean;
  installed: boolean;
}

export type ConversationStatus = "in_progress" | "requires_action" | "failed" | "done";

export interface Conversation {
  id: string;
  workspaceId: string | null;
  title: string;
  createdAt: number;
  updatedAt: number;
  lastSeenAt: number;
  status: ConversationStatus;
}

export interface Message {
  id: string;
  conversationId: string;
  role: "user" | "assistant" | "tool";
  contentType: "text" | "tool_call" | "tool_result" | "error" | "rich_text" | "context_notice";
  content: string;
  toolName: string | null;
  createdAt: number;
  durationMs: number | null;
  /** 010-context-window-management (UI refactor): input tokens for a user
   * message, output tokens for an assistant reply — real tokenizer count,
   * frozen at persistence time (mirrors durationMs). */
  tokenCount: number | null;
}

// 004-tool-call-widgets: a `tool_result` message's `content` (JSON string)
// parses into one of these, discriminated on `toolName` — see
// specs/004-tool-call-widgets/data-model.md for the authoritative shapes.
// Each variant is self-sufficient (arguments + outcome together) so a
// widget renders from this one row alone, no lookup of its paired
// `tool_call` row needed.

export type ReadOutcome =
  | { ok: true; content: string; truncated: boolean }
  | { ok: false; error: string };

export interface ReadDetail {
  toolName: "Read";
  filePath: string | null;
  offset: number | null;
  limit: number | null;
  outcome: ReadOutcome;
  /** 010-context-window-management/US3: set when this result was large
   * enough to be offloaded to disk — the model saw only a preview, but the
   * full content is still readable from this path. */
  offloadedTo?: string | null;
  /** Real tokenizer count of this result's content — see
   * `context::annotate_with_token_count` on the backend. */
  tokenCount?: number;
}

export type WriteOutcome = { ok: true } | { ok: false; error: string };

export interface WriteDetail {
  toolName: "Write";
  filePath: string | null;
  contentPreview: string;
  byteCount: number;
  outcome: WriteOutcome;
}

export type EditOutcome = { ok: true } | { ok: false; error: string };

export interface EditDetail {
  toolName: "Edit";
  filePath: string | null;
  oldString: string;
  newString: string;
  replaceAll: boolean;
  outcome: EditOutcome;
}

export type BashOutcome =
  | { ok: true; exitCode: number; stdout: string; stderr: string }
  | { ok: false; error: string };

export interface BashDetail {
  toolName: "Bash";
  command: string | null;
  timeoutMs: number | null;
  /** Absent while the command is still running — see BashWidget's pending
   * branch, fed by `parsePendingBashCallDetail`. */
  outcome?: BashOutcome;
  /** 010-context-window-management/US3: set when this result was large
   * enough to be offloaded to disk — the model saw only a preview, but the
   * full stdout/stderr is still readable from this path. */
  offloadedTo?: string | null;
  /** Real tokenizer count of this result's model-facing text — see
   * `context::annotate_with_token_count` on the backend. Only ever set for
   * Read/Bash/Grep/Glob (the four tools whose size varies enough to make a
   * cost badge worth showing). */
  tokenCount?: number;
}

export interface GlobDetail {
  toolName: "Glob";
  pattern: string | null;
  path: string | null;
  matches: string[];
  /** Real tokenizer count of this result's model-facing text — see
   * `context::annotate_with_token_count` on the backend. Only ever set for
   * Read/Bash/Grep/Glob (the four tools whose size varies enough to make a
   * cost badge worth showing). */
  tokenCount?: number;
}

export interface GrepMatch {
  path: string;
  lineNumber: number;
  line: string;
}

export interface GrepDetail {
  toolName: "Grep";
  pattern: string | null;
  path: string | null;
  glob: string | null;
  matches: GrepMatch[];
  /** Real tokenizer count of this result's model-facing text — see
   * `context::annotate_with_token_count` on the backend. Only ever set for
   * Read/Bash/Grep/Glob (the four tools whose size varies enough to make a
   * cost badge worth showing). */
  tokenCount?: number;
}

export interface TaskDetail {
  toolName: "Task";
  prompt: string;
  subagentConversationId: string;
  state: "running" | "complete";
}

export interface QuestionOption {
  label: string;
  description: string;
}

export interface AskUserQuestionDetail {
  toolName: "AskUserQuestion";
  questionId: string;
  header: string;
  question: string;
  options: QuestionOption[];
  multiSelect: boolean;
  answer: string[] | null;
}

export interface UnknownToolDetail {
  toolName: string;
  arguments: unknown;
  outcome: { ok: boolean; text: string };
}

// Deliberately NOT including UnknownToolDetail in this union: its
// `toolName: string` is non-literal, so merging it in would defeat switch
// narrowing on `toolName` for every other variant (TS can't exclude a
// non-literal-discriminant member from a specific literal case). Callers
// that render "the known shapes, or the fallback" use
// `ToolResultDetail | UnknownToolDetail` instead.
export type ToolResultDetail =
  | ReadDetail
  | WriteDetail
  | EditDetail
  | BashDetail
  | GlobDetail
  | GrepDetail
  | TaskDetail
  | AskUserQuestionDetail;

const KNOWN_TOOL_NAMES = new Set([
  "Read",
  "Write",
  "Edit",
  "Bash",
  "Glob",
  "Grep",
  "Task",
  "AskUserQuestion",
]);

/** Parses a `tool_result` message's `content`, degrading to the fallback shape on any parse failure or unrecognized `toolName` (data-model.md's Validation rules) rather than throwing into the message list. */
export function parseToolResultDetail(
  content: string,
  toolName: string | null,
): ToolResultDetail | UnknownToolDetail {
  try {
    const parsed = JSON.parse(content) as { toolName?: unknown };
    if (parsed && typeof parsed.toolName === "string" && KNOWN_TOOL_NAMES.has(parsed.toolName)) {
      return parsed as ToolResultDetail;
    }
    return {
      toolName: toolName ?? "Unknown",
      arguments: parsed,
      outcome: { ok: false, text: content },
    };
  } catch {
    return {
      toolName: toolName ?? "Unknown",
      arguments: null,
      outcome: { ok: false, text: content },
    };
  }
}

/** Parses a still-*pending* `AskUserQuestion` tool_call row's `content`
 * (shape `{"arguments": {header, question, options, multiSelect,
 * questionId}}` -- `persist_tool_call`'s generic wrapper around whatever
 * arguments a tool call carries) into the same `AskUserQuestionDetail`
 * shape the *answered* widget already renders from, with `answer: null`.
 * Returns `null` on any parse failure or missing `questionId` (an older
 * tool_call row from before this field existed, or plain corruption)
 * rather than throwing -- there is simply nothing answerable to show. */
export function parseAskUserQuestionCallDetail(content: string): AskUserQuestionDetail | null {
  try {
    const parsed = JSON.parse(content) as { arguments?: Record<string, unknown> };
    const args = parsed?.arguments;
    if (!args || typeof args.questionId !== "string" || typeof args.question !== "string") {
      return null;
    }
    return {
      toolName: "AskUserQuestion",
      questionId: args.questionId,
      header: typeof args.header === "string" ? args.header : "",
      question: args.question,
      options: Array.isArray(args.options) ? (args.options as QuestionOption[]) : [],
      multiSelect: args.multiSelect === true,
      answer: null,
    };
  } catch {
    return null;
  }
}

/** Parses a still-*pending* `Bash` tool_call row's `content` (shape
 * `{"arguments": {command, timeoutMs}}`) into an outcome-less `BashDetail`
 * — `BashWidget` treats a missing `outcome` as "still running." Returns
 * `null` on any parse failure or missing `command`. */
export function parsePendingBashCallDetail(content: string): BashDetail | null {
  try {
    const parsed = JSON.parse(content) as { arguments?: Record<string, unknown> };
    const args = parsed?.arguments;
    if (!args || typeof args.command !== "string") {
      return null;
    }
    return {
      toolName: "Bash",
      command: args.command,
      timeoutMs: typeof args.timeoutMs === "number" ? args.timeoutMs : null,
    };
  } catch {
    return null;
  }
}

/** Parses a still-*pending* `Task` tool_call row's `content` (shape
 * `{"arguments": {prompt}}`) into a `state: "running"` `TaskDetail` —
 * `TaskWidget` already has a dedicated running-state render branch for
 * this value, previously never actually produced (the backend only ever
 * persisted `tool_result` once the subagent had already finished, so
 * `state` was always `"complete"`). `subagentConversationId` isn't known
 * yet at this point (the subagent hasn't been spawned) — empty string is
 * safe since `TaskWidget` never renders it. Returns `null` on any parse
 * failure or missing `prompt`. */
export function parsePendingTaskCallDetail(content: string): TaskDetail | null {
  try {
    const parsed = JSON.parse(content) as { arguments?: Record<string, unknown> };
    const args = parsed?.arguments;
    if (!args || typeof args.prompt !== "string") {
      return null;
    }
    return {
      toolName: "Task",
      prompt: args.prompt,
      subagentConversationId: "",
      state: "running",
    };
  } catch {
    return null;
  }
}

// 009-rich-chat-input/US2 — a `rich_text` message's `content` (JSON string)
// parses into this shape (see specs/009-rich-chat-input/data-model.md's
// "Frontend Types" section for the authoritative shapes). Mirrors the Rust
// `RichMessageContent`/`RichTextSegment` types exactly.

export interface RichTextSegmentText {
  type: "text";
  text: string;
}

export interface RichTextSegmentPastedText {
  type: "pastedText";
  id: string;
  text: string;
  lineCount: number;
}

export interface RichTextSegmentAttachment {
  type: "attachment";
  id: string;
  name: string;
  mimeType: string;
  data: string;
  isImage: boolean;
}

export interface RichTextSegmentSkill {
  type: "skill";
  id: string;
  name: string;
}

export type RichTextSegment =
  | RichTextSegmentText
  | RichTextSegmentPastedText
  | RichTextSegmentAttachment
  | RichTextSegmentSkill;

export interface RichMessageContent {
  segments: RichTextSegment[];
}

export interface SendMessageResult {
  messageId: string;
  requestId: string;
  assistantMessageId: string;
  assistantCreatedAt: number;
}

export interface SearchResult {
  conversationId: string;
  title: string;
  excerpt: string;
  rank: number;
}

export interface Workspace {
  id: string;
  path: string;
  displayName: string;
  createdAt: number;
  lastOpenedAt: number;
}

export interface FolderSearchResult {
  path: string;
  displayName: string;
}

export interface FolderSearchPage {
  folders: FolderSearchResult[];
  truncated: boolean;
}

export interface McpServerConnection {
  id: string;
  name: string;
  transport: string;
  config: string;
  enabled: boolean;
  createdAt: number;
}

export interface McpToolInfo {
  name: string;
  description: string | null;
}

export interface SkillSummary {
  name: string;
  description: string;
}

// 009-rich-chat-input/US4 — result of reading a user-selected file's bytes
// for attachment (contracts/rich-chat-input.md's `read_attached_file`).
// `data` is base64, no `data:` prefix — matches the `attachment` segment's
// `data` field shape in data-model.md.
export interface AttachedFile {
  data: string;
  mimeType: string;
  name: string;
}

// 010-context-window-management — mirrors src-tauri/src/context/mod.rs's
// `ContextUsage`/`ContextState` and data-model.md's `ContextNoticeDetail`.
export type ContextState = "normal" | "warning" | "justCompacted";

export interface ContextUsage {
  conversationId: string;
  tokensUsed: number;
  tokenBudget: number;
  state: ContextState;
}

export type ContextNoticeDetail =
  | { kind: "cleared"; clearedCount: number; notice: string }
  | { kind: "summarized"; summary: string; notice: string };

/** Parses a `context_notice` message's `content`, degrading to a plain-text
 * notice on any parse failure rather than throwing (mirrors
 * `parseToolResultDetail`'s degrade-gracefully convention). */
export function parseContextNoticeDetail(content: string): ContextNoticeDetail {
  try {
    const parsed = JSON.parse(content) as { kind?: unknown };
    if (parsed.kind === "cleared" || parsed.kind === "summarized") {
      return parsed as ContextNoticeDetail;
    }
  } catch {
    // fall through to the degraded shape below
  }
  return { kind: "cleared", clearedCount: 0, notice: content };
}

export const commands = {
  getHardwareProfile: () => invoke<HardwareProfile>("get_hardware_profile"),
  startModelInstall: (modelId?: string) =>
    invoke<StartModelInstallResult>("start_model_install", { modelId }),
  getModelInstallStatus: (modelId: string) =>
    invoke<{ state: string; bytesDownloaded: number; bytesTotal: number }>(
      "get_model_install_status",
      { modelId },
    ),
  listModels: () => invoke<ModelRow[]>("list_models"),
  setActiveModel: (modelId: string) => invoke<void>("set_active_model", { modelId }),
  createConversation: (workspaceId?: string) =>
    invoke<Conversation>("create_conversation", { workspaceId }),
  sendMessage: (conversationId: string, content: string, richContent?: string) =>
    invoke<SendMessageResult>("send_message", { conversationId, content, richContent }),
  listConversations: (workspaceId?: string) =>
    invoke<Conversation[]>("list_conversations", { workspaceId }),
  listMessages: (conversationId: string) => invoke<Message[]>("list_messages", { conversationId }),
  markConversationSeen: (conversationId: string) =>
    invoke<void>("mark_conversation_seen", { conversationId }),
  archiveConversation: (conversationId: string) =>
    invoke<void>("archive_conversation", { conversationId }),
  searchConversations: (query: string) => invoke<SearchResult[]>("search_conversations", { query }),
  // Values cross as JSON-encoded strings (see commands/settings.rs for why)
  // — parse/stringify at the call site.
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  updateSetting: (key: string, valueJson: string) =>
    invoke<void>("update_setting", { key, valueJson }),
  setFocusedConversation: (conversationId: string | null) =>
    invoke<void>("set_focused_conversation", { conversationId }),
  cancelGeneration: (requestId: string) => invoke<boolean>("cancel_generation", { requestId }),
  openWorkspace: (path: string) => invoke<Workspace>("open_workspace", { path }),
  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),
  searchFolders: (query: string, maxResults?: number) =>
    invoke<FolderSearchPage>("search_folders", { query, maxResults }),
  sendAgentMessage: (conversationId: string, content: string, richContent?: string) =>
    invoke<string>("send_agent_message", { conversationId, content, richContent }),
  answerUserQuestion: (questionId: string, answer: string[]) =>
    invoke<void>("answer_user_question", { questionId, answer }),
  addMcpServer: (name: string, command: string, args: string[]) =>
    invoke<McpServerConnection>("add_mcp_server", { name, command, args }),
  listMcpServers: () => invoke<McpServerConnection[]>("list_mcp_servers"),
  listMcpServerTools: (serverId: string) =>
    invoke<McpToolInfo[]>("list_mcp_server_tools", { serverId }),
  listSkills: () => invoke<SkillSummary[]>("list_skills"),
  readAttachedFile: (path: string) => invoke<AttachedFile>("read_attached_file", { path }),
  getContextUsage: (conversationId: string) =>
    invoke<ContextUsage>("get_context_usage", { conversationId }),
  compactConversation: (conversationId: string) =>
    invoke<ContextUsage>("compact_conversation", { conversationId }),
};

export interface ModelInstallProgressPayload {
  modelId: string;
  bytesDownloaded: number;
  bytesTotal: number;
  state: string;
}

export interface AssistantTokenPayload {
  conversationId: string;
  messageId: string;
  token: string;
}

export interface AssistantMessageCompletePayload {
  conversationId: string;
  messageId: string;
  durationMs: number;
  tokenCount: number | null;
}

export interface AssistantMessageErrorPayload {
  conversationId: string;
  messageId: string;
  error: string;
}

export type GenerationQueueState = "queued" | "generating";

export interface GenerationQueueUpdatePayload {
  requestId: string;
  conversationId: string;
  state: GenerationQueueState;
  position: number | null;
}

// 004-tool-call-widgets/US3 — the one live event this feature adds
// (contracts/tool-widgets.md; research.md § 3).
export interface AskUserQuestionEventPayload {
  conversationId: string;
  questionId: string;
  header: string;
  question: string;
  options: QuestionOption[];
  multiSelect: boolean;
}

/** Streaming (loop-level, not token-level): fired every time a new row is
 * persisted for `conversationId` during an agent turn — a tool_call, its
 * paired tool_result, or the final answer — so the frontend can re-fetch
 * `list_messages` and re-render as the loop progresses, instead of waiting
 * for `send_agent_message`'s single promise to resolve at the very end. */
export interface AgentMessagePersistedPayload {
  conversationId: string;
}

export const events = {
  onModelInstallProgress: (cb: (p: ModelInstallProgressPayload) => void): Promise<UnlistenFn> =>
    listen<ModelInstallProgressPayload>("model-install-progress", (e) => cb(e.payload)),
  onAskUserQuestion: (cb: (p: AskUserQuestionEventPayload) => void): Promise<UnlistenFn> =>
    listen<AskUserQuestionEventPayload>("ask-user-question", (e) => cb(e.payload)),
  onAssistantToken: (cb: (p: AssistantTokenPayload) => void): Promise<UnlistenFn> =>
    listen<AssistantTokenPayload>("assistant-token", (e) => cb(e.payload)),
  onAssistantMessageComplete: (
    cb: (p: AssistantMessageCompletePayload) => void,
  ): Promise<UnlistenFn> =>
    listen<AssistantMessageCompletePayload>("assistant-message-complete", (e) => cb(e.payload)),
  onAssistantMessageError: (cb: (p: AssistantMessageErrorPayload) => void): Promise<UnlistenFn> =>
    listen<AssistantMessageErrorPayload>("assistant-message-error", (e) => cb(e.payload)),
  onGenerationQueueUpdate: (cb: (p: GenerationQueueUpdatePayload) => void): Promise<UnlistenFn> =>
    listen<GenerationQueueUpdatePayload>("generation-queue-update", (e) => cb(e.payload)),
  onContextUsageUpdate: (cb: (p: ContextUsage) => void): Promise<UnlistenFn> =>
    listen<ContextUsage>("context-usage-update", (e) => cb(e.payload)),
  onAgentMessagePersisted: (cb: (p: AgentMessagePersistedPayload) => void): Promise<UnlistenFn> =>
    listen<AgentMessagePersistedPayload>("agent-message-persisted", (e) => cb(e.payload)),
};
