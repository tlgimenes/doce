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
  status: ConversationStatus;
}

export interface Message {
  id: string;
  conversationId: string;
  role: "user" | "assistant" | "tool";
  contentType: "text" | "tool_call" | "tool_result" | "error";
  content: string;
  toolName: string | null;
  createdAt: number;
  durationMs: number | null;
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
  sendMessage: (conversationId: string, content: string) =>
    invoke<SendMessageResult>("send_message", { conversationId, content }),
  listConversations: (workspaceId?: string) =>
    invoke<Conversation[]>("list_conversations", { workspaceId }),
  listMessages: (conversationId: string) =>
    invoke<Message[]>("list_messages", { conversationId }),
  searchConversations: (query: string) =>
    invoke<SearchResult[]>("search_conversations", { query }),
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
  sendAgentMessage: (conversationId: string, content: string) =>
    invoke<string>("send_agent_message", { conversationId, content }),
  addMcpServer: (name: string, command: string, args: string[]) =>
    invoke<McpServerConnection>("add_mcp_server", { name, command, args }),
  listMcpServers: () => invoke<McpServerConnection[]>("list_mcp_servers"),
  listMcpServerTools: (serverId: string) =>
    invoke<McpToolInfo[]>("list_mcp_server_tools", { serverId }),
  listSkills: () => invoke<SkillSummary[]>("list_skills"),
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

export const events = {
  onModelInstallProgress: (cb: (p: ModelInstallProgressPayload) => void): Promise<UnlistenFn> =>
    listen<ModelInstallProgressPayload>("model-install-progress", (e) => cb(e.payload)),
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
};
