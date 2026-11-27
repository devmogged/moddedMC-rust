import { useEffect, useRef, useState } from "react";
import banner from "~/assets/intro-banner.png";
import pumpkin from "~/assets/intro-pumpkin.svg";
import { api } from "~/api";
import { formatBytes } from "~/lib/hooks";
import { getSettings } from "~/settings";

const inTauri = "__TAURI_INTERNALS__" in window;
/** Long enough for the intro to play once, short enough not to get in the way. */
const MIN_INTRO_MS = 2100;

export type LaunchResult = { runtime: string | null; notice: string | null };

/**
 * The launch screen: the logo animates in while a bar along the bottom checks for an app update,
 * installs or updates the server files, and starts this PC's agent. Then it hands over.
 */
export function Launch({ onDone }: { onDone: (result: LaunchResult) => void }) {
  const [step, setStep] = useState("Starting");
  const [progress, setProgress] = useState<number | null>(null);
  const [leaving, setLeaving] = useState(false);
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    const intro = new Promise((r) => setTimeout(r, getSettings().reduceMotion ? 0 : MIN_INTRO_MS));
    (async () => {
      const result = await run(setStep, setProgress);
      await intro;
      setLeaving(true);
      setTimeout(() => onDone(result), getSettings().reduceMotion ? 0 : 280);
    })();
  }, [onDone]);

  return (
    <div
      data-tauri-drag-region
      className={`fixed inset-0 z-50 flex flex-col items-center justify-center bg-background transition-opacity duration-300 ${leaving ? "opacity-0" : ""}`}
    >
      <div data-tauri-drag-region className="flex flex-col items-center gap-5">
        {/* The pumpkin lands, then the MODDED banner slams onto it and the pumpkin takes the hit. */}
        <div className="relative size-44">
          <div className="absolute inset-0 animate-[launch-glow_2.4s_ease-in-out_infinite] rounded-full bg-primary/30 blur-3xl" />
          <div className="absolute inset-0 animate-[launch-shake_320ms_ease-out_1100ms_both]">
            <img
              src={pumpkin}
              alt=""
              draggable={false}
              className="absolute inset-0 size-full animate-[launch-drop_700ms_cubic-bezier(0.34,1.56,0.64,1)_both]"
            />
            <div className="absolute inset-x-6 bottom-6 h-6 animate-[launch-dust_500ms_ease-out_1100ms_both] rounded-full bg-white/50 blur-md" />
            <img
              src={banner}
              alt="moddedMC"
              draggable={false}
              className="absolute inset-0 size-full animate-[launch-slam_380ms_cubic-bezier(0.5,0,0.75,0)_720ms_both]"
            />
          </div>
        </div>
        <div className="animate-[launch-fade_600ms_ease-out_1300ms_both] text-center">
          <div className="font-semibold text-2xl tracking-tight">moddedMC</div>
          <div className="text-muted-foreground text-sm">Modded Minecraft, fast.</div>
        </div>
      </div>
      <div className="absolute inset-x-0 bottom-0 flex flex-col gap-2 px-8 pb-6">
        <div className="flex justify-between text-muted-foreground text-xs">
          <span>{step}</span>
          {progress !== null && <span className="tabular-nums">{Math.round(progress * 100)}%</span>}
        </div>
        <div className="h-1 overflow-hidden rounded-full bg-muted">
          {progress === null ? (
            <div className="h-full w-1/3 animate-[launch-indeterminate_1.1s_ease-in-out_infinite] rounded-full bg-primary" />
          ) : (
            <div
              className="h-full rounded-full bg-primary transition-[width] duration-200"
              style={{ width: `${progress * 100}%` }}
            />
          )}
        </div>
      </div>
    </div>
  );
}

async function run(
  setStep: (s: string) => void,
  setProgress: (p: number | null) => void,
): Promise<LaunchResult> {
  if (!inTauri) return { runtime: null, notice: null };

  if (getSettings().autoUpdate) {
    setStep("Checking for updates");
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update) {
        setStep(`Updating moddedMC to ${update.version}`);
        let total = 0;
        let got = 0;
        await update.downloadAndInstall((e) => {
          if (e.event === "Started") total = e.data.contentLength ?? 0;
          if (e.event === "Progress") {
            got += e.data.chunkLength;
            if (total) setProgress(got / total);
          }
        });
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      }
    } catch {
      // No release yet, or offline: carry on with this version.
    }
    setProgress(null);
  }

  setStep("Checking server files");
  let runtime: string | null = null;
  let notice: string | null = null;
  try {
    const status = await api.runtimeStatus();
    runtime = status.installed;
    if (status.latest && status.latest !== status.installed) {
      setStep(status.installed ? `Updating server files to ${status.latest}` : "Installing server files");
      const off = await api.onRuntimeProgress((e) => {
        setStep(
          `${status.installed ? "Updating" : "Downloading"} the ${e.role} · ${formatBytes(e.received)} of ${formatBytes(e.total)}`,
        );
        setProgress(e.total ? e.received / e.total : null);
      });
      try {
        runtime = await api.installRuntime();
      } finally {
        off();
        setProgress(null);
      }
    } else if (!status.installed) {
      notice = `Server files aren't installed: ${status.error ?? "no release found"}`;
    }
  } catch (err) {
    notice = `Couldn't set up the server files: ${err instanceof Error ? err.message : String(err)}`;
  }

  if (runtime) {
    setStep("Starting the agent");
    try {
      await api.ensureAgent();
    } catch (err) {
      notice = err instanceof Error ? err.message : String(err);
    }
  }
  setStep("Ready");
  return { runtime, notice };
}
