import type { ReactNode } from "react";
import { api, type Device, type ServerInfo } from "~/api";
import { formatBytes, formatDuration, usePoll } from "~/lib/hooks";
import { cn } from "~/lib/utils";

export function OverviewPanel({ device, server }: { device: Device; server: ServerInfo }) {
  const live = server.state === "running" || server.state === "starting";
  const stats = usePoll(live ? () => api.stats(device.id, server.name) : null, 2000, [
    device.id,
    server.name,
    live,
  ]);
  const s = live ? stats.value : null;
  const h = s?.health ?? server.health;
  const tpsTone = !h
    ? undefined
    : h.tps >= h.tickrate * 0.95
      ? "good"
      : h.tps >= h.tickrate * 0.75
        ? "warn"
        : "bad";

  return (
    <div className="flex flex-col gap-6 p-6">
      {!live && (
        <p className="rounded-xl border bg-card p-4 text-muted-foreground text-sm">
          {server.state === "crashed"
            ? `The server crashed${server.exit_code != null ? ` (exit code ${server.exit_code})` : ""}. Check the console, then start it again.`
            : "The server is stopped. Start it to see players and performance."}
        </p>
      )}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
        <Stat label="Players" value={h ? `${h.players} / ${h.max_players}` : "–"} />
        <Stat
          label="Ticks per second"
          value={h ? h.tps.toFixed(1) : "–"}
          tone={tpsTone}
          hint={h ? `target ${h.tickrate}` : undefined}
        />
        <Stat
          label="Tick time"
          value={h ? `${h.mspt.toFixed(1)} ms` : "–"}
          hint={h ? `max ${h.mspt_max.toFixed(1)} ms` : undefined}
        />
        <Stat label="Memory" value={formatBytes(s?.rss_bytes)} />
        <Stat label="CPU" value={s?.cpu_percent != null ? `${s.cpu_percent.toFixed(0)}%` : "–"} />
        <Stat label="Uptime" value={formatDuration(s?.uptime_secs ?? server.uptime_secs)} />
      </div>
      <dl className="grid grid-cols-[8rem_1fr] gap-y-2 text-sm">
        <dt className="text-muted-foreground">Device</dt>
        <dd>{device.name}</dd>
        <dt className="text-muted-foreground">Port</dt>
        <dd>{server.port ?? 25565}</dd>
        {server.pack && (
          <>
            <dt className="text-muted-foreground">Pack</dt>
            <dd>{server.pack}</dd>
          </>
        )}
        {server.folder && (
          <>
            <dt className="text-muted-foreground">Folder</dt>
            <dd className="truncate select-text">{server.folder}</dd>
          </>
        )}
      </dl>
    </div>
  );
}

function Stat({
  label,
  value,
  hint,
  tone,
}: {
  label: string;
  value: ReactNode;
  hint?: string;
  tone?: "good" | "warn" | "bad";
}) {
  return (
    <div className="rounded-xl border bg-card p-4">
      <div className="text-muted-foreground text-xs">{label}</div>
      <div
        className={cn(
          "mt-1 font-semibold text-2xl tabular-nums tracking-tight",
          tone === "good" && "text-success-foreground",
          tone === "warn" && "text-warning-foreground",
          tone === "bad" && "text-destructive-foreground",
        )}
      >
        {value}
      </div>
      {hint && <div className="mt-0.5 text-muted-foreground text-xs">{hint}</div>}
    </div>
  );
}
