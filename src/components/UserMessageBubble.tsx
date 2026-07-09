import MarkdownPreview from "@/components/MarkdownPreview";
import { formatTokenCount } from "@/lib/formatTokenCount";
import { cn } from "@/lib/cn";
import type * as React from "react";
import type { Message } from "@/lib/ipc";
import UserMessageContent from "@/views/chat/rich-input/UserMessageContent";

export interface UserMessageBubbleProps {
  message: Message;
  bubbleClassName?: string;
  bubbleProps?: Omit<React.HTMLAttributes<HTMLDivElement>, "children" | "className">;
  bubbleTestId?: string;
  tokenMeterClassName?: string;
}

export default function UserMessageBubble({
  message,
  bubbleClassName,
  bubbleProps,
  bubbleTestId = "user-message-bubble",
  tokenMeterClassName,
}: UserMessageBubbleProps): React.JSX.Element {
  return (
    <>
      {message.contentType === "rich_text" ? (
        <div
          {...bubbleProps}
          className={cn(
            "prose prose-sm dark:prose-invert max-w-none rounded-lg bg-muted p-3 text-foreground",
            bubbleClassName,
          )}
          data-testid={bubbleTestId}
        >
          <UserMessageContent content={message.content} />
        </div>
      ) : (
        <MarkdownPreview
          {...bubbleProps}
          className={cn("rounded-lg bg-muted p-3 text-foreground", bubbleClassName)}
          testId={bubbleTestId}
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
