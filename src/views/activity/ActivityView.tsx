import { useCallback, useEffect, useMemo, useState } from "react";
import { Calendar, FileText, Mail, Terminal } from "lucide-react";
import { commands, events, type FeedCard } from "@/lib/ipc";
import { formatConversationRelativeTime } from "@/views/chat/sidebarConversationRow";
import ActivityFeed from "./ActivityFeed";
import ActivityCard, { type ActivityCardProps } from "./ActivityCard";

/** Logo glyph per card kind — monochrome, inheriting the card's ink. */
const KIND_LOGO: Record<FeedCard["kind"], React.ReactNode> = {
  draft: <Mail className="size-4" />,
  event: <Calendar className="size-4" />,
  file: <FileText className="size-4" />,
  shell: <Terminal className="size-4" />,
};

/**
 * Maps a persisted `FeedCard` onto the presentational `ActivityCard`. The
 * kind maps 1:1 onto the card variant. Only the Dismiss affordance is wired
 * (Discard for a draft, Dismiss elsewhere) — the service-specific commit
 * actions (Send email / Confirm event) are DEFERRED until real, human-gated
 * service tools exist, so they render inert for now (informational cards).
 */
function toCardProps(card: FeedCard, now: number, onDismiss: () => void): ActivityCardProps {
  const common = {
    logo: KIND_LOGO[card.kind],
    title: card.title,
    meta: card.sourceTool,
    timestamp: formatConversationRelativeTime(card.createdAt, now),
  };
  switch (card.kind) {
    case "draft":
      // Discard IS the dismiss affordance for a draft; Send/Edit deferred.
      return { ...common, kind: "draft", bodyPreview: card.preview, onDiscard: onDismiss };
    case "event":
      // No time slots surfaced yet (Confirm/Other time deferred); Dismiss only.
      return { ...common, kind: "event", slots: [], onDismiss };
    case "file":
      return { ...common, kind: "file", onDismiss };
    case "shell":
      return { ...common, kind: "shell", onDismiss };
  }
}

export interface ActivityViewProps {
  /** Scope the feed to one conversation; omit for the global feed. */
  conversationId?: string;
}

/**
 * The Activity surface: a persisted, dismissable feed of the actions the
 * agent took through connected MCP services. ADDITIVE — a standalone section
 * (rendered inside Settings), NOT a replacement for the chat transcript.
 *
 * Loads cards on mount, live-appends on the `feed-card-created` event, and
 * removes a card optimistically on Dismiss (calling `dismissFeedCard`).
 * Pending cards float to "Needs you"; dismissed ones fall to "Earlier".
 */
export default function ActivityView({ conversationId }: ActivityViewProps) {
  const [cards, setCards] = useState<FeedCard[]>([]);
  // A single "now" per render so every relative timestamp is consistent.
  const now = useMemo(() => Date.now(), [cards]);

  const refresh = useCallback(() => {
    void commands
      .listFeedCards(conversationId)
      .then(setCards)
      .catch(() => {
        // Best-effort: a transient IPC failure just leaves the last list;
        // the next event or refresh reconciles it.
      });
  }, [conversationId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Live-append: prepend a newly-created card (dedup by id in case a refresh
  // raced the event). Respect the conversation scope when one is set.
  useEffect(() => {
    const unlisten = events.onFeedCardCreated((card) => {
      if (conversationId && card.conversationId !== conversationId) return;
      setCards((previous) =>
        previous.some((existing) => existing.id === card.id) ? previous : [card, ...previous],
      );
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [conversationId]);

  const dismiss = useCallback((id: string) => {
    // Optimistic: flip to dismissed locally, then persist. A failed persist
    // leaves the optimistic state; the next refresh reconciles from the DB.
    setCards((previous) =>
      previous.map((card) => (card.id === id ? { ...card, status: "dismissed" } : card)),
    );
    void commands.dismissFeedCard(id).catch(() => {});
  }, []);

  const pending = cards.filter((card) => card.status === "pending");
  const dismissed = cards.filter((card) => card.status === "dismissed");

  return (
    <div data-testid="activity-view">
      <ActivityFeed
        needsYou={pending.map((card) => (
          <ActivityCard key={card.id} {...toCardProps(card, now, () => dismiss(card.id))} />
        ))}
        earlier={dismissed.map((card) => (
          <ActivityCard key={card.id} {...toCardProps(card, now, () => dismiss(card.id))} />
        ))}
        emptyState={
          <p
            data-testid="activity-empty"
            className="rounded-xl border border-dashed border-border bg-card/50 px-4 py-6 text-center text-sm text-muted-foreground"
          >
            No activity yet — connect a service and the agent's actions will show up here.
          </p>
        }
      />
    </div>
  );
}
