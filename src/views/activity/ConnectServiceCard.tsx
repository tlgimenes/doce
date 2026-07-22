import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { GoogleGIcon } from "./icons";

export interface ConnectServiceCardProps {
  /** Monochrome service glyph (or the Google mark for the Google row). */
  icon: ReactNode;
  name: string;
  /** One-line value proposition. */
  description: string;
  /** Button copy — defaults to "Connect". */
  actionLabel?: string;
  /**
   * "google" prefixes the button with the brand "G" and uses a neutral
   * outline treatment (Google guidelines); "plain" is a bare enable button.
   */
  brand?: "google" | "plain";
  /** Not yet available — renders a dimmed, disabled "Soon"-style row. */
  disabled?: boolean;
  /**
   * Home-Stream emphasis: a faint accent tint marking this as the one live
   * call to action in the feed. Off in Settings, where connect is one row
   * among many.
   */
  emphasis?: boolean;
  onConnect?: () => void;
}

/**
 * Empty-state / onboarding row: a service to connect. Logo, name, a
 * one-line value prop, and a single Connect/Enable button. Presentational —
 * `onConnect` is wired by the caller; the backend that performs the OAuth
 * does not exist yet.
 */
export default function ConnectServiceCard({
  icon,
  name,
  description,
  actionLabel = "Connect",
  brand = "plain",
  disabled = false,
  emphasis = false,
  onConnect,
}: ConnectServiceCardProps) {
  return (
    <div
      data-testid="connect-service-card"
      className={cn(
        "flex items-center gap-3 rounded-xl border p-3.5 shadow-sm",
        emphasis ? "border-emerald-500/40 bg-emerald-500/[0.06]" : "border-border bg-card",
        disabled && "opacity-55",
      )}
    >
      <span className="grid size-9 shrink-0 place-items-center rounded-lg border border-border bg-muted text-foreground">
        {icon}
      </span>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-semibold">{name}</div>
        <div className="text-xs text-muted-foreground">{description}</div>
      </div>
      {disabled ? (
        <Button type="button" variant="outline" size="sm" disabled>
          Soon
        </Button>
      ) : (
        <Button type="button" variant="outline" size="sm" onClick={onConnect}>
          {brand === "google" && <GoogleGIcon />}
          {actionLabel}
        </Button>
      )}
    </div>
  );
}
