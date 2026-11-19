import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { api, minecraftAddress, type Device, type Health, type ServerInfo } from "./api";

/* ---------- polling ---------- */

/** Runs `load` now and every `ms` while mounted; keeps the last value and error. */
export function usePoll<T>(load: (() => Promise<T>) | null, ms: number, deps: unknown[]) {
  const [value, setValue] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!load) return;
    let alive = true;
    let timer: number | undefined;
    const run = async () => {
      try {
        const next = await load();
        if (!alive) return;
        setValue(next);
        setError(null);
      } catch (err) {
        if (!alive) return;
        setError(err instanceof Error ? err.message : String(err));
      }
      if (alive) timer = window.setTimeout(run, ms);
    };
    run();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);
  return { value, error, refresh: () => setTick((t) => t + 1) };
}

/* ---------- server icon ---------- */

// Generated icons are ore blocks: a stone with specks of one ore, both picked from the name.
// Stones: stone, deepslate, sandstone, netherrack, end stone, tuff, calcite.
const STONES: [number, number, number][] = [
  [0.55, 0.005, 250],
  [0.36, 0.012, 260],
  [0.78, 0.06, 88],
  [0.4, 0.09, 22],
  [0.86, 0.05, 105],
  [0.47, 0.012, 150],
  [0.88, 0.01, 250],
];
// Ores: gold, diamond, emerald, redstone, lapis, copper, amethyst.
const ORES: [number, number, number][] = [
  [0.84, 0.15, 90],
  [0.83, 0.12, 190],
  [0.72, 0.18, 150],
  [0.6, 0.22, 28],
  [0.5, 0.15, 265],
  [0.68, 0.13, 50],
  [0.66, 0.15, 305],
];

function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const oklch = (l: number, c: number, h: number) => `oklch(${l.toFixed(3)} ${c} ${h})`;

function iconHue(image: HTMLImageElement): number | undefined {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 16;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.drawImage(image, 0, 0, 16, 16);
  const pixels = ctx.getImageData(0, 0, 16, 16).data;
  const bins = Array.from({ length: 12 }, () => ({ weight: 0, a: 0, b: 0 }));
  const linear = (channel: number) => {
    const v = channel / 255;
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  for (let i = 0; i < pixels.length; i += 4) {
    if (pixels[i + 3] < 128) continue;
    const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]].map(linear);
    // Linear sRGB to OKLab. Weight the dominant hue by chroma.
    const l = Math.cbrt(0.41222 * r + 0.53633 * g + 0.05145 * b);
    const m = Math.cbrt(0.2119 * r + 0.6807 * g + 0.1074 * b);
    const s = Math.cbrt(0.0883 * r + 0.28172 * g + 0.62998 * b);
    const a = 1.978 * l - 2.42859 * m + 0.45059 * s;
    const z = 0.0259 * l + 0.78277 * m - 0.80868 * s;
    const chroma = Math.hypot(a, z);
    if (chroma < 0.025) continue;
    const hue = ((Math.atan2(z, a) * 180) / Math.PI + 360) % 360;
    const bin = bins[Math.round(hue / 30) % bins.length];
    bin.weight += chroma;
    bin.a += a;
    bin.b += z;
  }
  const dominant = bins.reduce((best, bin) => (bin.weight > best.weight ? bin : best));
  return dominant.weight ? ((Math.atan2(dominant.b, dominant.a) * 180) / Math.PI + 360) % 360 : undefined;
}

export function useIcon(name: string, favicon?: string | null) {
  const [sample, setSample] = useState<{ source: string; hue: number | undefined; valid: boolean }>();
  const localImage = favicon?.startsWith("data:image/png;base64,") ? favicon : null;
  useEffect(() => {
    if (!localImage) return;
    let active = true;
    const img = new Image();
    img.src = localImage;
    img
      .decode()
      .then(() => {
        if (active) setSample({ source: localImage, hue: iconHue(img), valid: true });
      })
      .catch(() => {
        if (active) setSample({ source: localImage, hue: undefined, valid: false });
      });
    return () => {
      active = false;
    };
  }, [localImage]);
  const stone = STONES[hash(name + "#stone") % STONES.length];
  const ore = ORES[hash(name + "#ore") % ORES.length];
  const match = sample && sample.source === localImage ? sample : null;
  return {
    favicon: match?.valid ? localImage : null,
    style: { "--server-hue": match?.hue ?? (stone[1] >= 0.085 ? stone[2] : ore[2]) } as CSSProperties,
  };
}

const icons = new Map<string, Promise<string | null>>();

export function useServerIcon(device: Device, server: ServerInfo) {
  const address = minecraftAddress(device, server);
  const [resolved, setResolved] = useState<{ address: string; favicon: string | null }>();
  useEffect(() => {
    let active = true;
    let request = icons.get(address);
    if (!request && server.state === "running") {
      request = api
        .ping(address)
        .then((status) => status.favicon)
        .catch(() => {
          icons.delete(address);
          return null;
        });
      icons.set(address, request);
    }
    request?.then((favicon) => {
      if (active) setResolved({ address, favicon });
    });
    return () => {
      active = false;
    };
  }, [address, server.state]);
  return useIcon(server.name, resolved?.address === address ? resolved.favicon : null);
}

/** An 8x8 ore block, stable for a name. Server icons are the only pixel art in the app. */
export function ServerIcon({ name, favicon }: { name: string; favicon?: string | null }) {
  const cells = useMemo(() => {
    const next = rng(hash(name));
    const [sl, sc, sh] = STONES[hash(name + "#stone") % STONES.length];
    const [ol, oc, oh] = ORES[hash(name + "#ore") % ORES.length];
    const grid: string[] = [];
    for (let i = 0; i < 64; i++) {
      const shade = [-0.06, -0.03, 0, 0, 0.03, 0.05][Math.floor(next() * 6)];
      grid.push(oklch(sl + shade, sc, sh));
    }
    // Three veins of two to four pixels, each with a lit and a shaded pixel like vanilla ores.
    for (let vein = 0; vein < 3; vein++) {
      let x = 1 + Math.floor(next() * 6);
      let y = 1 + Math.floor(next() * 6);
      const size = 2 + Math.floor(next() * 3);
      for (let p = 0; p < size; p++) {
        const lift = p === 0 ? 0.07 : p === size - 1 ? -0.08 : 0;
        grid[y * 8 + x] = oklch(ol + lift, oc, oh);
        if (next() < 0.5) x = Math.min(7, Math.max(0, x + (next() < 0.5 ? -1 : 1)));
        else y = Math.min(7, Math.max(0, y + (next() < 0.5 ? -1 : 1)));
      }
    }
    return grid.map((fill, i) => ({ x: i % 8, y: Math.floor(i / 8), fill }));
  }, [name]);
  if (favicon) return <img className="server-icon" src={favicon} alt="" draggable={false} />;
  return (
    <svg className="server-icon" viewBox="0 0 8 8" shapeRendering="crispEdges" aria-hidden>
      {cells.map((cell) => (
        <rect key={`${cell.x}-${cell.y}`} x={cell.x} y={cell.y} width="1" height="1" fill={cell.fill} />
      ))}
    </svg>
  );
}

/* ---------- bars ---------- */

export type BarsTone = "ok" | "unknown" | "warn" | "bad" | "starting" | "external";

export function Bars({
  lit,
  tone,
  label,
  crashed = false,
}: {
  lit: number;
  tone: BarsTone;
  label: string;
  crashed?: boolean;
}) {
  return (
    <span
      className="bars"
      data-crashed={crashed || undefined}
      data-tone={tone}
      role="img"
      aria-label={label}
      title={label}
    >
      {[0, 1, 2, 3, 4].map((i) => (
        <i key={i} style={{ "--i": i } as React.CSSProperties} data-on={i < lit ? "" : undefined} />
      ))}
    </span>
  );
}

/** The multiplayer list's latency thresholds. */
export function pingBars(ms: number): number {
  if (ms < 150) return 5;
  if (ms < 300) return 4;
  if (ms < 600) return 3;
  if (ms < 1000) return 2;
  return 1;
}

/**
 * Tick health as the list's five bars: all five while ticks use under half their budget,
 * then one fewer at 80% and at 100% of it, two when TPS falls under 75% of the tick rate.
 */
export function healthBars(health: Health): { lit: number; tone: BarsTone } {
  const budget = 1000 / health.tickrate;
  if (health.tps < health.tickrate * 0.75) return { lit: 1, tone: "bad" };
  if (health.mspt >= budget) return { lit: 2, tone: "warn" };
  if (health.mspt >= budget * 0.8) return { lit: 3, tone: "warn" };
  if (health.mspt >= budget * 0.5) return { lit: 4, tone: "ok" };
  return { lit: 5, tone: "ok" };
}

/** The status cluster for a hosted server: tick health and players while it runs. */
export function ServerCluster({
  device,
  server,
  detail = false,
}: {
  device: Device;
  server: ServerInfo;
  detail?: boolean;
}) {
  const live = useLive(device, server);
  switch (server.state) {
    case "running": {
      const bars = live.health
        ? healthBars(live.health)
        : { lit: 5, tone: (server.external ? "external" : "ok") as BarsTone };
      const label = live.health
        ? `${live.health.mspt.toFixed(1)} ms per tick, ${live.health.tps.toFixed(1)} TPS`
        : "Running";
      return (
        <span className="cluster">
          <Bars lit={bars.lit} tone={bars.tone} label={label} />
          {detail ? (
            <span className="status-text" data-tone={bars.tone}>
              {server.external
                ? "External"
                : bars.tone === "warn" || bars.tone === "bad"
                  ? "Degraded"
                  : "Running"}
            </span>
          ) : null}
          {live.players != null && live.max != null ? (
            <span>
              {live.players}/{live.max}
            </span>
          ) : null}
        </span>
      );
    }
    case "starting":
      return (
        <span className="cluster">
          <Bars lit={0} tone="starting" label="Starting" />
          <span className="word status-text" data-tone="starting">
            Starting
          </span>
        </span>
      );
    case "stopping":
      return (
        <span className="cluster">
          <Bars lit={0} tone="starting" label="Stopping" />
          <span className="word status-text" data-tone="starting">
            Stopping
          </span>
        </span>
      );
    case "crashed":
      return (
        <span className="cluster">
          <Bars lit={1} tone="bad" label="Crashed" crashed />
          <span className="word status-text" data-tone="bad">
            Crashed
          </span>
        </span>
      );
    default:
      return (
        <span className="cluster">
          <span className="word status-text" data-tone={server.external ? "external" : "unknown"}>
            Stopped
          </span>
        </span>
      );
  }
}

/* ---------- dialog ---------- */

export function Dialog({
  open,
  onClose,
  children,
}: {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      onClose={onClose}
      onMouseDown={(e) => {
        if (e.target === ref.current) onClose();
      }}
    >
      {open ? children : null}
    </dialog>
  );
}

/* ---------- formatting ---------- */

export function bytes(n: number | null | undefined): string {
  if (n == null) return "–";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

export function duration(secs: number | null | undefined): string {
  if (secs == null) return "–";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${Math.floor(secs)}s`;
}

/** A server row's second line: its pack, or what an external server reports. */
export function packLine(server: ServerInfo): string {
  if (server.external) return server.pack ? `External · ${server.pack}` : "External";
  return server.pack ?? "No pack";
}

export const routeLabel = { local: "Local", tailscale: "Tailscale", ssh: "SSH" } as const;

/**
 * Health and players for a running server. Pack servers report both to the agent; any other
 * server gets the game's server-list ping for its player count.
 */
export function useLive(device: Device, server: ServerInfo) {
  const health = server.state === "running" ? server.health : null;
  const address = server.state === "running" && !health ? minecraftAddress(device, server) : null;
  const { value } = usePoll(address ? () => api.ping(address) : null, 5000, [address]);
  if (health) return { health, players: health.players, max: health.max_players };
  const ping = address ? value : null;
  return { health: null, players: ping?.players_online ?? null, max: ping?.players_max ?? null };
}

export function LoadingRows() {
  return (
    <div className="skeleton" role="status" aria-label="Loading">
      <i />
      <i />
      <i />
    </div>
  );
}

export function LoadError({ error, retry }: { error: string; retry: () => void }) {
  return (
    <div className="state-panel error-line" role="status">
      <span>{error}</span>
      <button className="button" onClick={retry}>
        Retry
      </button>
    </div>
  );
}
