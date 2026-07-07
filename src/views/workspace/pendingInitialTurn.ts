import type { RichMessageContent } from "@/lib/ipc";

export interface PendingInitialTurn {
  conversationId: string;
  content: string;
  richContent?: RichMessageContent;
}
