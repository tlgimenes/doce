import { useRef, useState, type FocusEvent } from "react";
import type * as React from "react";
import UserMessageBubble from "@/components/UserMessageBubble";
import { cn } from "@/lib/cn";
import type { Message } from "@/lib/ipc";

export interface StickyUserMessageProps {
  message: Message;
  onScrollToTurn?: () => void;
}

export default function StickyUserMessage({
  message,
  onScrollToTurn,
}: StickyUserMessageProps): React.JSX.Element {
  const [expanded, setExpanded] = useState(false);
  const pointerOriginRef = useRef(false);

  const requestScroll = () => {
    onScrollToTurn?.();
  };

  const expandAndScrollOnFocus = (event: FocusEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    setExpanded(true);
    if (pointerOriginRef.current) return;
    requestScroll();
  };

  const expandAndScrollOnClick = () => {
    setExpanded(true);
    requestScroll();
    pointerOriginRef.current = false;
  };

  const collapseIfFocusLeft = (event: FocusEvent<HTMLDivElement>) => {
    const nextTarget = event.relatedTarget;

    if (nextTarget instanceof Node && event.currentTarget.contains(nextTarget)) return;
    setExpanded(false);
    pointerOriginRef.current = false;
  };

  const bubbleClassName = cn(
    "focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
    expanded
      ? "max-h-[50vh] overflow-auto opacity-100"
      : "max-h-[84px] overflow-hidden opacity-99 [mask-image:linear-gradient(to_bottom,black_calc(100%-16px),transparent)]",
  );

  return (
    <div
      aria-label="You said"
      className="sticky top-4 z-40 mb-8 sm:mb-6"
      data-sticky-user-message="true"
      data-testid="chat-message"
      role="group"
    >
      <UserMessageBubble
        message={message}
        bubbleClassName={bubbleClassName}
        bubbleProps={{
          onBlur: collapseIfFocusLeft,
          onClick: expandAndScrollOnClick,
          onFocus: expandAndScrollOnFocus,
          onPointerDown: () => {
            pointerOriginRef.current = true;
          },
          tabIndex: 0,
        }}
        bubbleTestId="sticky-user-message-bubble"
      />
    </div>
  );
}
