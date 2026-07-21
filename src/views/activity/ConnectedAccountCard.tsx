import { createElement } from "react";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { GoogleGIcon } from "./icons";
import type { GoogleService } from "./services";

export interface ConnectedAccountCardProps {
  /** The connected Google account address. */
  email: string;
  /** Display name; falls back to the email's local part. */
  name?: string;
  avatarUrl?: string;
  /** The services this account grants the agent. */
  services: GoogleService[];
  onDisconnect?: () => void;
}

/**
 * A connected Google account: avatar + email, then the services it grants
 * listed as STATIC rows (icon · name · scope caption · tool count). There
 * are deliberately NO per-service enable/disable toggles — once a service
 * is connected it is always available to the agent — so the only control is
 * a single "Disconnect account" action.
 */
export default function ConnectedAccountCard({
  email,
  name,
  avatarUrl,
  services,
  onDisconnect,
}: ConnectedAccountCardProps) {
  const initial = (name ?? email).trim().charAt(0).toUpperCase() || "?";

  return (
    <div
      data-testid="connected-account-card"
      className="flex flex-col gap-3 rounded-xl border border-border bg-card p-3.5 shadow-sm"
    >
      <div className="flex items-center gap-3">
        <div className="relative shrink-0">
          <Avatar>
            {avatarUrl && <AvatarImage src={avatarUrl} alt="" />}
            <AvatarFallback>{initial}</AvatarFallback>
          </Avatar>
          <span className="absolute -right-0.5 -bottom-0.5 grid size-4 place-items-center rounded-full bg-background ring-1 ring-border">
            <GoogleGIcon size={10} />
          </span>
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold">{name ?? email.split("@")[0]}</div>
          <div className="truncate text-xs text-muted-foreground">{email}</div>
        </div>
        <Button type="button" variant="ghost" size="sm" onClick={onDisconnect}>
          Disconnect account
        </Button>
      </div>

      <Separator />

      <ul className="flex flex-col">
        {services.map((service) => (
          <li
            key={service.id}
            data-testid="granted-service-row"
            className="flex items-center gap-3 py-2"
          >
            <span className="grid size-8 shrink-0 place-items-center rounded-lg border border-border bg-muted text-foreground">
              {createElement(service.icon, { size: 15 })}
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-medium">{service.name}</div>
              <div className="text-xs text-muted-foreground">{service.scope}</div>
            </div>
            <span className="shrink-0 text-xs text-muted-foreground">
              {service.toolCount} {service.toolCount === 1 ? "tool" : "tools"}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
