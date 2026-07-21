import { ArrowUp, Pencil, Trash2 } from "lucide-react";
import type { QueuedMessage } from "./messageQueueRegistry";

interface QueuedMessagesProps {
  items: readonly QueuedMessage[];
  /** "Send now" — steer this message into the running turn. */
  onSteer: (item: QueuedMessage) => void;
  /** Recall this message back into the composer for editing (removes the row). */
  onEdit: (item: QueuedMessage) => void;
  /** Remove this message from the queue without sending or steering it. */
  onDelete: (id: string) => void;
  /** Subtle inline error (e.g. a steer the running turn refused). */
  steerError?: string | null;
}

/**
 * Preview rows for messages queued while a turn is in flight, rendered between
 * the streaming status and the composer. Each row shows a truncated preview and
 * three controls: "Send now" (steer into the running turn), edit (recall into
 * the composer), and delete. Goal-mode rows HIDE "Send now" — the steer command
 * carries no goal intent, so a goal message can only drain as its own turn.
 * Renders nothing when the queue is empty.
 */
export default function QueuedMessages({
  items,
  onSteer,
  onEdit,
  onDelete,
  steerError,
}: QueuedMessagesProps) {
  if (items.length === 0) return null;

  return (
    <div className="flex flex-col gap-1" data-testid="queued-messages">
      {items.map((item) => {
        const preview = previewText(item);
        return (
          <div
            key={item.id}
            className="flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-1.5 text-xs text-muted-foreground"
            data-testid="queued-message-row"
          >
            <span className="min-w-0 flex-1 truncate">
              <span className="font-medium text-foreground">Queued</span> {preview}
            </span>
            {/* Goal-mode rows can't be steered (no goal flag on the steer path),
                so "Send now" is hidden — they drain as their own goal turn. */}
            {!item.setGoal && (
              <button
                type="button"
                onClick={() => onSteer(item)}
                className="shrink-0 rounded p-1 hover:bg-muted hover:text-foreground"
                aria-label="Send now"
                data-testid="queued-message-send-now"
              >
                <ArrowUp size={12} />
              </button>
            )}
            <button
              type="button"
              onClick={() => onEdit(item)}
              className="shrink-0 rounded p-1 hover:bg-muted hover:text-foreground"
              aria-label="Edit"
              data-testid="queued-message-edit"
            >
              <Pencil size={12} />
            </button>
            <button
              type="button"
              onClick={() => onDelete(item.id)}
              className="shrink-0 rounded p-1 hover:bg-destructive/10 hover:text-destructive"
              aria-label="Remove"
              data-testid="queued-message-delete"
            >
              <Trash2 size={12} />
            </button>
          </div>
        );
      })}
      {steerError && (
        <p className="px-1 text-xs text-destructive" data-testid="queue-steer-error">
          {steerError}
        </p>
      )}
    </div>
  );
}

/**
 * A short human preview of a queued message. Plain text uses the flat content;
 * a message that is entirely chips (empty flat text) falls back to a segment
 * count so the row is never blank.
 */
function previewText(item: QueuedMessage): string {
  const flat = item.content.trim();
  if (flat) return flat;
  const segmentCount = item.richContent?.segments.length ?? 0;
  return segmentCount === 1 ? "1 attachment" : `${segmentCount} attachments`;
}
