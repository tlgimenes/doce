import { useEffect, useRef, useState } from "react";
import { commands, events, type HardwareProfile } from "@/lib/ipc";
import logo from "@/assets/logo.png";

interface OnboardingProps {
  onReady: () => void;
}

type Phase = "detecting" | "downloading" | "verifying" | "preparing" | "active" | "error";

/**
 * FR-001–FR-004: zero-config first run. No model picker, no API key, no
 * account — hardware detection and model download happen automatically.
 */
export default function Onboarding({ onReady }: OnboardingProps) {
  const [profile, setProfile] = useState<HardwareProfile | null>(null);
  const [phase, setPhase] = useState<Phase>("detecting");
  const [bytesDownloaded, setBytesDownloaded] = useState(0);
  const [bytesTotal, setBytesTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const started = useRef(false);

  useEffect(() => {
    // Guards against React StrictMode's deliberate double-invocation of
    // effects in development, which otherwise fires two concurrent
    // start_model_install calls racing on the same .part file (caught via
    // real e2e testing: the downloaded file came out at ~2x expected size).
    if (started.current) return;
    started.current = true;

    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        const hw = await commands.getHardwareProfile();
        setProfile(hw);

        const sub = await events.onModelInstallProgress((p) => {
          setBytesDownloaded(p.bytesDownloaded);
          setBytesTotal(p.bytesTotal);
          if (p.state === "downloading") setPhase("downloading");
          if (p.state === "verifying") setPhase("verifying");
          if (p.state === "preparing" || p.state === "downloaded") setPhase("preparing");
          // Downloaded bytes are not enough: enter the app only after the
          // supervised server has loaded and health-checked the model and the
          // global active pointer has committed.
          if (p.state === "active") {
            setPhase("active");
            onReady();
          }
          // Without this, a failed download/verification (checksum
          // mismatch, network error, disk full) left the UI stuck on
          // "Downloading…"/"Verifying…" forever with no feedback — found
          // via e2e testing hanging indefinitely with no error shown.
          if (p.state.startsWith("error")) {
            setError(p.state);
            setPhase("error");
          }
        });
        unlisten = sub;

        setPhase("downloading");
        await commands.startModelInstall();
      } catch (e) {
        setError(String(e));
        setPhase("error");
      }
    })();

    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pct = bytesTotal > 0 ? Math.round((bytesDownloaded / bytesTotal) * 100) : 0;

  return (
    <div className="flex h-dvh flex-col items-center justify-center gap-6 bg-background px-6 text-center text-foreground">
      <img src={logo} alt="doce" className="h-24 w-auto" />
      <h1 className="text-balance text-2xl font-semibold">doce</h1>
      {profile && (
        <p className="text-sm text-muted-foreground">
          {profile.chip} · {profile.ramGb}GB · tier {profile.tier}
        </p>
      )}
      {(phase === "downloading" || phase === "verifying" || phase === "preparing") && (
        <div className="w-64">
          <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
            <div
              className="h-full w-full origin-left bg-primary transition-transform duration-300 ease-out"
              style={{ transform: `scaleX(${pct / 100})` }}
            />
          </div>
          <p className="mt-2 text-center text-xs tabular-nums text-muted-foreground">
            {phase === "downloading"
              ? `Downloading model… ${pct}%`
              : phase === "verifying"
                ? `Verifying… ${pct}%`
                : "Getting the model ready…"}
          </p>
        </div>
      )}
      {phase === "error" && <p className="text-sm text-destructive">{error}</p>}
    </div>
  );
}
