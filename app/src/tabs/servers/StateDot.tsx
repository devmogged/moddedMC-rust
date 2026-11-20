import type { ServerState } from "~/api";
import { cn } from "~/lib/utils";

const COLORS: Record<ServerState, string> = {
  running: "bg-emerald-500",
  starting: "bg-amber-400 animate-pulse",
  stopping: "bg-amber-400 animate-pulse",
  stopped: "bg-zinc-500",
  crashed: "bg-red-500",
};

export const STATE_LABEL: Record<ServerState, string> = {
  running: "Running",
  starting: "Starting",
  stopping: "Stopping",
  stopped: "Stopped",
  crashed: "Crashed",
};

export function StateDot({ state, className }: { state: ServerState; className?: string }) {
  return (
    <span
      aria-label={STATE_LABEL[state]}
      className={cn("size-2 shrink-0 rounded-full", COLORS[state], className)}
    />
  );
}
