import { useId, useState, type CSSProperties } from "react";
import { bytes } from "./ui";
import type { DeviceInfo } from "./api";

export function DeviceMeters({ info }: { info: DeviceInfo }) {
  const ram = info.ram_total > 0 ? 100 * (1 - info.ram_available / info.ram_total) : null;
  return (
    <div className="device-meters">
      <span title={info.cpu_percent == null ? "CPU thread count" : "CPU usage"}>
        CPU <b>{info.cpu_percent == null ? info.cpus + "t" : Math.round(info.cpu_percent) + "%"}</b>
        {info.cpu_percent != null && <Meter label="CPU usage" value={info.cpu_percent} />}
      </span>
      <span title={bytes(info.ram_available) + " free of " + bytes(info.ram_total)}>
        RAM <b>{ram == null ? "–" : Math.round(ram) + "%"}</b>
        {ram != null && <Meter label="RAM used" value={ram} />}
      </span>
      <span title={bytes(info.disk_free) + " disk space free"}>
        Disk <b>{bytes(info.disk_free)}</b>
        {info.disk_total != null && info.disk_total > 0 && (
          <Meter label="Disk used" value={100 * (1 - info.disk_free / info.disk_total)} />
        )}
      </span>
    </div>
  );
}

function Meter({ label, value }: { label: string; value: number }) {
  return (
    <meter
      aria-label={label}
      min={0}
      max={100}
      high={85}
      optimum={0}
      value={Math.min(100, Math.max(0, value))}
    />
  );
}

type Trace = {
  values: (number | null)[];
  min?: number;
  max?: number;
  tone?: string;
};

const W = 240;
const H = 72;
const GUTTER = 4;
const SLOTS = 120;

export function HistoryPlot({
  values,
  min = 0,
  max = 0,
  tone,
  label,
  format,
}: Trace & {
  label?: string;
  format?: (value: number) => string;
}) {
  const id = useId();
  const [index, setIndex] = useState<number | null>(null);
  const usable = values.filter((v): v is number => v != null);
  const upper = Math.max(max, ...usable);
  const lower = Math.min(min, ...usable);
  const span = upper - lower || 1;
  const plot = values.map((value, i) => {
    const slots = Math.max(values.length, label ? 12 : 5);
    const x = GUTTER + (1 - (values.length - 1 - i) / (slots - 1)) * (W - 2 * GUTTER);
    const y = value == null ? null : H - GUTTER - ((value - lower) / span) * (H - 2 * GUTTER);
    return { x, y, value };
  });
  const segments: { x: number; y: number }[][] = [];
  let segment: { x: number; y: number }[] = [];
  for (const { x, y } of plot) {
    if (y == null) {
      segment = [];
      continue;
    }
    if (!segment.length) segments.push(segment);
    segment.push({ x, y });
  }

  const selected = index == null ? null : plot[Math.min(index, plot.length - 1)];
  const point = selected?.value != null && selected.y != null ? selected : null;
  const last = plot.at(-1);
  return (
    <div
      className={label ? "history-plot chart" : "history-plot sparkline"}
      data-tone={tone}
      tabIndex={label && usable.length ? 0 : undefined}
      role={label ? "group" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      onFocus={label ? () => setIndex(plot.length - 1) : undefined}
      onBlur={label ? () => setIndex(null) : undefined}
      onPointerMove={
        label
          ? (event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              const x = ((event.clientX - rect.left) / rect.width) * W;
              const nearest = plot.reduce(
                (best, p, i) => (Math.abs(p.x - x) < Math.abs(plot[best].x - x) ? i : best),
                0,
              );
              setIndex(nearest);
            }
          : undefined
      }
      onPointerLeave={
        label
          ? (event) => {
              if (document.activeElement !== event.currentTarget) setIndex(null);
            }
          : undefined
      }
      onKeyDown={
        label
          ? (event) => {
              if (event.key === "Escape") {
                setIndex(null);
                return;
              }
              if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              setIndex((current) => {
                const last = plot.length - 1;
                if (event.key === "Home") return 0;
                if (event.key === "End") return last;
                return Math.max(0, Math.min(last, (current ?? last) + (event.key === "ArrowLeft" ? -1 : 1)));
              });
            }
          : undefined
      }
    >
      <svg viewBox={"0 0 " + W + " " + H} preserveAspectRatio="none" aria-hidden>
        <defs>
          <linearGradient id={id} x2="0" y2="1">
            <stop offset="0" className="trace-fill" />
            <stop offset="1" className="trace-fill" stopOpacity="0.04" />
          </linearGradient>
        </defs>
        {(label ? [4, 25, 46, 68] : [68]).map((y) => (
          <line key={y} className="trace-grid" x1="0" x2={W} y1={y} y2={y} />
        ))}
        {segments.map((points, i) => {
          const first = points[0];
          const end = points[points.length - 1];
          // Repeat the first point outside the sampled range to keep SVG path commands interpolable.
          const coords = [
            ...Array.from({ length: (label ? SLOTS : 12) - points.length }, () => first),
            ...points,
          ];
          const line = "M " + coords.map(({ x, y }) => x.toFixed(2) + " " + y.toFixed(2)).join(" L ");
          const fill =
            line + " L " + end.x.toFixed(2) + " " + H + " L " + first.x.toFixed(2) + " " + H + " Z";
          return (
            <g key={i}>
              <path
                d={fill}
                fill={"url(#" + id + ")"}
                style={{ "--trace": 'path("' + fill + '")' } as CSSProperties}
              />
              <path
                className="trace-line"
                d={line}
                style={{ "--trace": 'path("' + line + '")' } as CSSProperties}
              />
            </g>
          );
        })}
        {point && <line className="trace-cursor" x1={point.x} x2={point.x} y1="0" y2={H} />}
      </svg>
      {!label && last?.y != null && (
        <i className="trace-tip" style={{ left: (100 * last.x) / W + "%", top: (100 * last.y) / H + "%" }} />
      )}
      {label && (
        <output
          className="trace-value"
          hidden={!selected}
          style={{
            right: selected && selected.x > W / 2 ? 100 - (100 * selected.x) / W + "%" : undefined,
            left: selected && selected.x <= W / 2 ? (100 * selected.x) / W + "%" : undefined,
          }}
        >
          {selected?.value != null ? format?.(selected.value) : "–"}
        </output>
      )}
    </div>
  );
}

export function PregenMap({ percent }: { percent: number | null }) {
  const progress = Math.min(100, Math.max(0, percent ?? 0));
  return (
    <div
      className="pregen-map"
      role="progressbar"
      aria-label="Pregeneration progress"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent == null ? undefined : progress}
    >
      {[1, 2, 3, 4].map((ring) => (
        <i key={ring} style={{ "--ring": ring } as CSSProperties} />
      ))}
      <span className="map-fill" style={{ "--extent": Math.sqrt(progress / 100) } as CSSProperties} />
      <i className="map-center" />
    </div>
  );
}
