import MarkdownPreview from "@/components/MarkdownPreview";
import { formatTokenCount } from "@/lib/formatTokenCount";
import { cn } from "@/lib/cn";
import type * as React from "react";
import type { Message } from "@/lib/ipc";
import UserMessageContent from "@/views/chat/rich-input/UserMessageContent";
import { Bubble, BubbleContent } from "@/components/ui/bubble";

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
      <Bubble align="end" variant="user" className="ml-auto max-w-[85%]">
        <BubbleContent
          {...bubbleProps}
          className={bubbleClasses}
          data-testid={bubbleTestId}
        >
          {message.contentType === "rich_text" ? (
            <div className="prose prose-sm max-w-none dark:prose-invert">
              <UserMessageContent content={message.content} />
            </div>
          ) : (
            <MarkdownPreview className="max-w-none p-0">{message.content}</MarkdownPreview>
          )}
        </BubbleContent>
      </Bubble>
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
