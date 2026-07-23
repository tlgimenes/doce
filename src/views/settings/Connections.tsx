import { useCallback, useEffect, useMemo, useState } from "react";
import { Check, ShieldCheck } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import ConnectServiceCard from "@/views/activity/ConnectServiceCard";
import ConnectedAccountCard from "@/views/activity/ConnectedAccountCard";
import { GoogleGIcon } from "@/views/activity/icons";
import { GOOGLE_SERVICES, type GoogleService } from "@/views/activity/services";
import {
  commands,
  type GoogleWorkspaceServiceInfo,
  type McpServerConnection,
  type OAuthAccount,
} from "@/lib/ipc";

/** Parses an MCP server's `{"url","oauth_account_id"}` config and returns the
 * linked account id, or null for a non-oauth server / malformed config. */
function oauthAccountIdOf(server: McpServerConnection): string | null {
  try {
    const parsed = JSON.parse(server.config) as { oauth_account_id?: unknown };
    return typeof parsed.oauth_account_id === "string" ? parsed.oauth_account_id : null;
  } catch {
    return null;
  }
}

/** "list" shows the empty/connected surface; "connecting" awaits the blocking
 * browser-consent command. There is no in-between form: doce ships a built-in
 * Google OAuth client, so connecting is one click that grants every service. */
type Phase = "list" | "connecting";

export interface ConnectionsProps {
  /**
   * "settings" (default) renders the full titled section — heading, privacy
   * note, and per-service account cards. "home" renders just the connect
   * card (or a slim connected chip) to sit inline in the empty-state Stream,
   * with no section chrome; account management stays in Settings.
   */
  surface?: "settings" | "home";
  /**
   * Fired on mount and whenever the connected-account count crosses zero, so
   * the home Stream can brighten its preview cards once a service connects.
   */
  onConnectionChange?: (connected: boolean) => void;
}

export default function Connections({
  surface = "settings",
  onConnectionChange,
}: ConnectionsProps = {}) {
  const isHome = surface === "home";
  const [accounts, setAccounts] = useState<OAuthAccount[]>([]);
  const [servers, setServers] = useState<McpServerConnection[]>([]);
  const [workspaceServices, setWorkspaceServices] = useState<GoogleWorkspaceServiceInfo[]>([]);

  const [phase, setPhase] = useState<Phase>("list");
  const [error, setError] = useState<string | null>(null);
  const [pendingDisconnect, setPendingDisconnect] = useState<OAuthAccount | null>(null);

  const refresh = useCallback(() => {
    void commands
      .listOauthAccounts()
      .then(setAccounts)
      .catch(() => {
        // Leave the last-known accounts in place; a later refresh reconciles.
      });
    void commands
      .listMcpServers()
      .then(setServers)
      .catch(() => {
        // Service cross-referencing degrades to "no rows" rather than blanking
        // the connected accounts themselves.
      });
  }, []);

  useEffect(() => {
    refresh();
    void commands
      .listGoogleWorkspaceServices()
      .then(setWorkspaceServices)
      .catch(() => {
        // Without the preset list we can't register servers after consent; the
        // connect still authenticates, just grants no services until reloaded.
      });
  }, [refresh]);

  // Report connected/not to the home Stream so it can brighten its preview
  // cards. Fires on mount (accounts start empty) and on every crossing.
  const connected = accounts.length > 0;
  useEffect(() => {
    onConnectionChange?.(connected);
  }, [connected, onConnectionChange]);

  // Map a preset's stable key (and its written display name) to the presentational
  // GoogleService, so the connected card can show each service's glyph + scope.
  const serviceByKey = useMemo(
    () => new Map(GOOGLE_SERVICES.map((service) => [service.id, service])),
    [],
  );
  const serviceByDisplayName = useMemo(() => {
    const map = new Map<string, GoogleService>();
    for (const info of workspaceServices) {
      const match = serviceByKey.get(info.key);
      if (match) map.set(info.displayName, match);
    }
    return map;
  }, [workspaceServices, serviceByKey]);

  // One click: authenticate with the built-in client (empty client_id → the
  // baked-in credentials), then grant EVERY Google service. No picker, no
  // credential fields.
  const connect = async () => {
    setError(null);
    setPhase("connecting");
    try {
      const account = await commands.connectOauthAccount("google", "", undefined, []);
      const keys = workspaceServices.map((service) => service.key);
      if (keys.length > 0) {
        await commands.addGoogleWorkspaceServers(account.id, keys);
      }
      refresh();
      setPhase("list");
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
      setPhase("list");
    }
  };

  const confirmDisconnect = async () => {
    const account = pendingDisconnect;
    setPendingDisconnect(null);
    if (!account) return;
    try {
      await commands.removeOauthAccount(account.id);
      refresh();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  };

  const servicesForAccount = (account: OAuthAccount): GoogleService[] =>
    servers
      .filter((server) => oauthAccountIdOf(server) === account.id)
      .map((server) => serviceByDisplayName.get(server.name))
      .filter((service): service is GoogleService => service !== undefined);

  const content = (
    <>
      {phase === "connecting" ? (
        <Card size="sm" data-testid="connect-waiting">
          <CardContent className="flex items-center gap-3">
            <Spinner />
            <div>
              <div className="text-sm font-medium">Approve doce in your browser</div>
              <div className="text-xs text-muted-foreground">
                This window updates automatically.
              </div>
            </div>
          </CardContent>
        </Card>
      ) : accounts.length === 0 ? (
        <div className="space-y-4">
          <ConnectServiceCard
            icon={<GoogleGIcon size={16} />}
            name="Google Workspace"
            description="Gmail, Calendar, and Drive — the agent triages and drafts, you approve."
            brand="google"
            emphasis={isHome}
            onConnect={() => void connect()}
          />
          {error ? (
            <p className="text-sm text-destructive" data-testid="connect-error">
              Couldn't connect: {error}. Please try again.
            </p>
          ) : null}
          {!isHome && <PrivacyNote />}
        </div>
      ) : isHome ? (
        // Home Stream: a slim connected chip, nothing to manage inline —
        // disconnect, per-service detail, and adding accounts all live in
        // Settings so the feed stays quiet once you're connected.
        <div className="flex flex-col gap-3">
          {accounts.map((account) => {
            const granted = servicesForAccount(account).length;
            return (
              <div
                key={account.id}
                data-testid="home-connected-chip"
                className="flex items-center gap-3 rounded-xl border border-emerald-500/30 bg-card p-3 shadow-sm"
              >
                <span className="grid size-8 shrink-0 place-items-center rounded-lg border border-border bg-muted text-foreground">
                  <GoogleGIcon size={15} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-semibold">Google Workspace</div>
                  <div className="text-xs text-muted-foreground">
                    Connected
                    {granted > 0 && ` · ${granted} ${granted === 1 ? "service" : "services"}`}
                  </div>
                </div>
                <Check className="size-4 shrink-0 text-emerald-600 dark:text-emerald-500" />
              </div>
            );
          })}
        </div>
      ) : (
        <div className="space-y-4">
          {accounts.map((account) => (
            <ConnectedAccountCard
              key={account.id}
              name="Google Workspace"
              email={`Connected ${new Date(account.createdAt).toLocaleDateString()}`}
              services={servicesForAccount(account)}
              onDisconnect={() => setPendingDisconnect(account)}
            />
          ))}
          {error ? (
            <p className="text-sm text-destructive" data-testid="connect-error">
              Couldn't connect: {error}. Please try again.
            </p>
          ) : null}
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void connect()}
            data-testid="connect-another"
          >
            <GoogleGIcon />
            Connect another account
          </Button>
          <PrivacyNote />
        </div>
      )}
    </>
  );

  // Home Stream: no section chrome — the connect card / chip flows inline in
  // the feed. Disconnect lives in Settings, so no confirmation dialog here.
  if (isHome) {
    return content;
  }

  return (
    <section
      aria-labelledby="connections-heading"
      data-testid="connections-section"
      className="space-y-4"
    >
      <div>
        <h4 id="connections-heading" className="text-sm font-medium">
          Connections
        </h4>
        <p className="mt-0.5 text-sm text-muted-foreground">
          Give the agent tools to work on your behalf. It reads and drafts through the local model
          on this Mac.
        </p>
      </div>

      {content}

      <AlertDialog
        open={pendingDisconnect !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDisconnect(null);
        }}
      >
        <AlertDialogContent data-testid="disconnect-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>Disconnect this account?</AlertDialogTitle>
            <AlertDialogDescription>
              The agent loses its Gmail, Calendar, and Drive tools until you reconnect. Your Google
              tokens are removed from this Mac.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep connected</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => void confirmDisconnect()}
              data-testid="confirm-disconnect"
            >
              Disconnect
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}

/** Local-first reassurance: the model reads your data on-device; only the
 * Google API calls it makes on your behalf leave the machine. */
function PrivacyNote() {
  return (
    <div
      data-testid="connections-privacy-note"
      className="flex items-start gap-2.5 rounded-lg border border-border bg-muted/40 p-3"
    >
      <ShieldCheck size={16} className="mt-0.5 shrink-0 text-muted-foreground" />
      <p className="text-xs text-muted-foreground">
        The local model reads your data on this Mac. Only the Google API calls the agent makes on
        your behalf leave the machine — nothing is sent to us.
      </p>
    </div>
  );
}
