import { useState } from "react";
import { Globe, Pause, Play, X } from "lucide-react";
import { api, type Device, type ServerInfo } from "~/api";
import { Button } from "~/components/ui/button";
import { Input } from "~/components/ui/input";
import { Switch } from "~/components/ui/switch";
import { errorText, formatDuration, usePoll } from "~/lib/hooks";
import { toast } from "~/lib/toast";

/** Pregenerating chunks ahead of time (the Chunky port), so exploring doesn't lag. */
export function WorldPanel({ device, server }: { device: Device; server: ServerInfo }) {
  const running = server.state === "running";
  const pregen = usePoll(running ? () => api.pregen(device.id, server.name) : null, 1500, [
    device.id,
    server.name,
    running,
  ]);
  const [radius, setRadius] = useState(1000);
  const [circle, setCircle] = useState(true);
  const p = pregen.value;
  const active = p && (p.state === "running" || p.state === "paused");

  const go = async (work: () => Promise<unknown>) => {
    try {
      await work();
      pregen.refresh();
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  if (!running)
    return <p className="p-6 text-muted-foreground text-sm">Start the server to pregenerate its world.</p>;

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="rounded-xl border bg-card p-4">
        <div className="flex items-center gap-2 font-medium text-sm">
          <Globe className="size-4" /> Pregenerate the world
        </div>
        <p className="mt-1 text-muted-foreground text-xs">
          Builds terrain ahead of time so players exploring don't cause lag. Uses every CPU core.
        </p>
        {active && p ? (
          <div className="mt-4 flex flex-col gap-3">
            <div className="h-2 overflow-hidden rounded-full bg-muted">
              <div className="h-full bg-primary transition-[width]" style={{ width: `${p.percent ?? 0}%` }} />
            </div>
            <div className="flex items-center justify-between text-sm">
              <span className="tabular-nums">
                {(p.percent ?? 0).toFixed(1)}% · {p.chunks?.toLocaleString() ?? 0} chunks
                {p.rate ? ` · ${Math.round(p.rate)}/s` : ""} · {formatDuration(p.eta_secs)} left
              </span>
              <div className="flex gap-2">
                {p.state === "running" ? (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => go(() => api.pregenAction(device.id, server.name, "pause"))}
                  >
                    <Pause /> Pause
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => go(() => api.pregenAction(device.id, server.name, "continue"))}
                  >
                    <Play /> Continue
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="ghost-destructive"
                  onClick={() => go(() => api.pregenAction(device.id, server.name, "cancel"))}
                >
                  <X /> Cancel
                </Button>
              </div>
            </div>
          </div>
        ) : (
          <div className="mt-4 flex flex-wrap items-center gap-4 text-sm">
            <label className="flex items-center gap-2">
              Radius
              <Input
                type="number"
                className="w-24 tabular-nums"
                value={radius}
                min={16}
                max={30000}
                onChange={(e) => setRadius(Math.max(16, Math.min(30000, Number(e.target.value) || 16)))}
              />
              blocks
            </label>
            <label className="flex items-center gap-2">
              <Switch checked={circle} onCheckedChange={setCircle} /> Circle
            </label>
            <Button
              className="ml-auto"
              onClick={() =>
                go(() =>
                  api.startPregen(device.id, server.name, { radius, center_x: 0, center_z: 0, circle }),
                )
              }
            >
              <Play /> Start
            </Button>
          </div>
        )}
        {p?.state === "finished" && (
          <p className="mt-3 text-success-foreground text-sm">Last run finished.</p>
        )}
      </div>
      {pregen.error && <p className="text-destructive-foreground text-sm">{pregen.error}</p>}
    </div>
  );
}
