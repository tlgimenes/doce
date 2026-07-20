import { Spinner } from "@/components/ui/spinner";
import logo from "@/assets/logo.png";

/**
 * The instant boot screen. Rendered the moment React mounts — before the
 * readiness check (App.tsx's `checkReadyWithRetries`) resolves — so a fresh
 * launch shows the brand immediately instead of a blank window while the
 * Tauri IPC bridge warms up and `getModelState` answers (the dominant
 * perceived-boot delay on a new machine).
 *
 * Its shell mirrors Onboarding's exactly (same centered logo + wordmark), so
 * when the check resolves to onboarding the brand stays put and only the
 * content below the wordmark swaps the spinner for the install progress —
 * no flash, no jump. When it resolves to the workspace, the splash is simply
 * replaced.
 */
export default function Splash() {
  return (
    <div
      className="flex h-dvh flex-col items-center justify-center gap-6 bg-background px-6 text-center text-foreground"
      data-testid="app-splash"
    >
      <img src={logo} alt="doce" className="h-24 w-auto" />
      <h1 className="text-balance text-2xl font-semibold">doce</h1>
      <Spinner className="size-5 text-muted-foreground" />
    </div>
  );
}
