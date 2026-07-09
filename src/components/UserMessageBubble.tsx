import MarkdownPreview from "@/components/MarkdownPreview";
import { formatTokenCount } from "@/lib/formatTokenCount";
import { cn } from "@/lib/cn";
import type { Message } from "@/lib/ipc";
import UserMessageContent from "@/views/chat/rich-input/UserMessageContent";

export interface UserMessageBubbleProps {
  message: Message;
  bubbleClassName?: string;
  tokenMeterClassName?: string;
}

export default function UserMessageBubble({
  message,
  bubbleClassName,
  tokenMeterClassName,
}: UserMessageBubbleProps) {
  return (
    <>
      {message.contentType === "rich_text" ? (
        <div
          className={cn(
            "prose prose-sm dark:prose-invert max-w-none rounded-lg bg-muted p-3 text-foreground",
            bubbleClassName,
          )}
          data-testid="user-message-bubble"
        >
          <UserMessageContent content={message.content} />
        </div>
      ) : (
        <MarkdownPreview
          className={cn("rounded-lg bg-muted p-3 text-foreground", bubbleClassName)}
          testId="user-message-bubble"
        >
          {message.content}
        </MarkdownPreview>
      )}
      {message.tokenCount != null && (
        <p
          className={cn("mt-1 text-xs text-muted-foreground", tokenMeterClassName)}
          data-testid="token-meter"
        >
          ↑ {formatTokenCount(message.tokenCount)} tokens
        </p>
      )}
    </>
  );
}
