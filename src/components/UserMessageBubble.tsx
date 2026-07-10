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
  const bubbleClasses = cn(
    "ml-auto max-w-[85%] rounded-md border border-border bg-[var(--color-doce-cream)] p-3 text-sm text-foreground shadow-sm",
    bubbleClassName,
  );

  return (
    <>
      {message.contentType === "rich_text" ? (
        <div
          {...bubbleProps}
          className={cn(bubbleClasses, "prose prose-sm dark:prose-invert")}
          data-testid={bubbleTestId}
        >
          <UserMessageContent content={message.content} />
        </div>
      ) : (
        <MarkdownPreview
          {...bubbleProps}
          className={bubbleClasses}
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
