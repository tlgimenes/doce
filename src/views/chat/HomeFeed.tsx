import { useState } from "react";
import { Calendar, Mail } from "lucide-react";
import Connections from "@/views/settings/Connections";
import ActivityView from "@/views/activity/ActivityView";
import PreviewCard from "@/views/activity/PreviewCard";

/**
 * The empty-state "Stream": one fluid column under the composer that dissolves
 * the old labeled Connections + Activity sections into a single feed. The
 * connect-Google surface is just the first card; below it, either the agent's
 * real action cards or — while the feed is empty — dashed preview cards that
 * brighten the moment a service connects. No section headers: the cards
 * describe themselves.
 */
export default function HomeFeed() {
  const [connected, setConnected] = useState(false);

  return (
    <div className="mt-10 w-full max-w-xl" data-testid="home-feed">
      <div className="flex flex-col gap-3">
        <Connections surface="home" onConnectionChange={setConnected} />
        <ActivityView emptyState={<HomePreview ready={connected} />} />
      </div>
    </div>
  );
}

/**
 * The empty feed's body: a plain lead line (the real, announced description)
 * plus decorative preview cards showing the shape of what's coming. `ready`
 * follows the connection state — dashed and dim before connecting, solid once
 * a service is linked and the agent just hasn't acted yet.
 */
function HomePreview({ ready }: { ready: boolean }) {
  return (
    <div className="flex flex-col gap-3" data-testid="home-preview">
      <p className="px-1 text-sm text-muted-foreground">
        {ready
          ? "Connected. As the agent works, its drafts and holds land here for you to approve."
          : "Once you connect, the agent's drafts and calendar holds show up here to approve."}
      </p>
      <PreviewCard
        ready={ready}
        logo={<Mail className="size-4" />}
        title="Draft replies to send"
        meta="Written for you — send or edit in a tap."
      />
      <PreviewCard
        ready={ready}
        logo={<Calendar className="size-4" />}
        title="Calendar holds to confirm"
        meta="Proposed times — one tap to accept."
      />
    </div>
  );
}
