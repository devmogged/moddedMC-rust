import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Archive, Grid3x3, History, Pause, Play, Plus, RotateCw, Square, Trash2, Upload } from "lucide-react";
import { api, minecraftAddress, type Backup, type BackupSchedule, type Device, type ServerInfo, type Stats } from "./api";
import { ServerCluster, ServerIcon, bytes, duration, packLine, usePoll } from "./ui";
import { ConfirmDialog, DeployDialog } from "./dialogs";

type Tab = "console" | "performance" | "mods" | "backups" | "pregen";

const TABS: { id: Tab; label: string }[] = [
  { id: "console", label: "Console" },
  { id: "performance", label: "Performance" },
  { id: "mods", label: "Mods" },
  { id: "backups", label: "Backups" },
  { id: "pregen", label: "Pregen" },
];

export function ServerPane({
  device,
  server,
  onChanged,
  onError,
}: {
  device: Device;
  server: ServerInfo;
  onChanged: () => void;
  onError: (err: unknown) => void;
}) {
  // Dev previews can open a tab directly: #server/pi/survival/performance.
  const [tab, setTab] = useState<Tab>(() => {
    const hashTab = location.hash.split("/")[3];
    return import.meta.env.DEV && TABS.some((t) => t.id === hashTab) ? (hashTab as Tab) : "console";
  });
  const [busy, setBusy] = useState(false);
  const [deploying, setDeploying] = useState(false);

  const act = async (action: "start" | "stop" | "restart") => {
    setBusy(true);
    try {
      await api.action(device.id, server.name, action);
    } catch (err) {
      onError(err);
    } finally {
      setBusy(false);
      onChanged();
    }
  };

  const up = server.state === "running";
  const moving = server.state === "starting" || server.state === "stopping";
  const line = [
    packLine(server),
    device.name,
    minecraftAddress(device, server),
    server.state === "crashed" && server.exit_code != null ? `Exit code ${server.exit_code}` : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className="pane">
      <header className="pane-head">
        <div className="row large">
          <ServerIcon name={server.name} />
          <div className="row-text">
            <h1 className="row-name">{server.name}</h1>
            <p className="row-line">{line}</p>
          </div>
          <ServerCluster device={device} server={server} />
        </div>
        <div className="actions">
          {server.external ? null : up || moving ? (
            <>
              <button className="button" disabled={busy || moving} onClick={() => act("restart")}>
                <RotateCw />
                Restart
              </button>
              <button className="button" disabled={busy || moving} onClick={() => act("stop")}>
                <Square />
                Stop
              </button>
            </>
          ) : (
            <>
              <button className="button" disabled={busy} onClick={() => setDeploying(true)}>
                <Upload />
                Deploy
              </button>
              <button className="button primary" disabled={busy} onClick={() => act("start")}>
                <Play />
                Start
              </button>
            </>
          )}
        </div>
      </header>

      {server.external ? (
        <div className="pane-body ruled">
          <dl className="facts">
            {server.program ? (
              <>
                <dt>Program</dt>
                <dd>{server.program}</dd>
              </>
            ) : null}
            {server.pid != null ? (
              <>
                <dt>Process</dt>
                <dd>{server.pid}</dd>
              </>
            ) : null}
            <dt>Address</dt>
            <dd>{minecraftAddress(device, server)}</dd>
            {server.folder ? (
              <>
                <dt>Folder</dt>
                <dd className="mono">{server.folder}</dd>
              </>
            ) : null}
          </dl>
        </div>
      ) : (
        <>
          <div className="tabs" role="tablist">
            {TABS.map((t) => (
              <button
                key={t.id}
                className="tab"
                role="tab"
                aria-selected={tab === t.id}
                onClick={() => setTab(t.id)}
              >
                {t.label}
              </button>
            ))}
          </div>
          {tab === "console" ? (
            <Console device={device.id} server={server.name} />
          ) : tab === "performance" ? (
            <Performance device={device.id} server={server} />
          ) : tab === "mods" ? (
            <Mods device={device.id} server={server} onError={onError} />
          ) : tab === "backups" ? (
            <Backups device={device.id} server={server} onError={onError} />
          ) : (
            <Pregen device={device.id} server={server} onError={onError} />
          )}
        </>
      )}

      <DeployDialog
        open={deploying}
        device={device}
        server={server.name}
        onClose={() => setDeploying(false)}
        onDeployed={onChanged}
      />
    </div>
  );
}

const MAX_LINES = 2000;

function level(line: string): string | undefined {
  if (/\b(ERROR|FATAL)\b|panicked/.test(line)) return "error";
  if (/\bWARN\b/.test(line)) return "warn";
  if (line.startsWith("> ")) return "echo";
  return undefined;
}

function Console({ device, server }: { device: string; server: string }) {
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [cursor, setCursor] = useState(-1);
  const idRef = useRef<number | null>(null);
  const logRef = useRef<HTMLDivElement>(null);
  const follow = useRef(true);

  useEffect(() => {
    let alive = true;
    let retry: number | undefined;
    const unlisten = api.onConsole(({ id, line }) => {
      if (id !== idRef.current) return;
      if (line === null) {
        idRef.current = null;
        retry = window.setTimeout(open, 2000);
        return;
      }
      setLines((prev) => {
        const next = prev.length >= MAX_LINES ? prev.slice(prev.length - MAX_LINES + 1) : prev.slice();
        next.push(line);
        return next;
      });
    });
    async function open() {
      try {
        const id = await api.consoleOpen(device, server);
        if (!alive) {
          api.consoleClose(id);
          return;
        }
        // The agent replays its backlog on every connect.
        setLines([]);
        setError(null);
        idRef.current = id;
      } catch (err) {
        if (!alive) return;
        setError((err as Error).message);
        retry = window.setTimeout(open, 3000);
      }
    }
    open();
    return () => {
      alive = false;
      window.clearTimeout(retry);
      unlisten.then((f) => f());
      if (idRef.current !== null) api.consoleClose(idRef.current);
      idRef.current = null;
    };
  }, [device, server]);

  useLayoutEffect(() => {
    const log = logRef.current;
    if (log && follow.current) log.scrollTop = log.scrollHeight;
  }, [lines]);

  const send = async () => {
    const command = input.trim();
    if (!command || idRef.current === null) return;
    setInput("");
    setCursor(-1);
    setHistory((h) => [command, ...h.filter((c) => c !== command)].slice(0, 100));
    follow.current = true;
    setLines((prev) => [...prev, `> ${command}`]);
    try {
      await api.consoleSend(idRef.current, command);
    } catch (err) {
      setError((err as Error).message);
    }
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      send();
    } else if (e.key === "ArrowUp" && history.length) {
      e.preventDefault();
      const next = Math.min(cursor + 1, history.length - 1);
      setCursor(next);
      setInput(history[next]);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      const next = cursor - 1;
      setCursor(Math.max(next, -1));
      setInput(next >= 0 ? history[next] : "");
    }
  };

  return (
    <div className="console">
      <div
        className="console-log"
        ref={logRef}
        role="log"
        onScroll={(e) => {
          const el = e.currentTarget;
          follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
      >
        {lines.map((line, i) => (
          <div key={i} data-level={level(line)}>
            {line}
          </div>
        ))}
        {error ? <div data-level="error">{error}</div> : null}
      </div>
      <label className="console-input">
        <span aria-hidden>&gt;</span>
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKey}
          placeholder="Command"
          spellCheck={false}
          autoComplete="off"
          aria-label="Console command"
        />
      </label>
    </div>
  );
}

function Mods({
  device,
  server,
  onError,
}: {
  device: string;
  server: ServerInfo;
  onError: (err: unknown) => void;
}) {
  const { value, error, refresh } = usePoll(() => api.serverMods(device, server.name), 10_000, [
    device,
    server.name,
  ]);
  const [busy, setBusy] = useState(false);
  const locked = server.state !== "stopped" && server.state !== "crashed";

  const change = async (run: () => Promise<void>) => {
    setBusy(true);
    try {
      await run();
    } catch (err) {
      onError(err);
    } finally {
      setBusy(false);
      refresh();
    }
  };

  const add = async () => {
    const path = await api.pickModFile();
    if (path) await change(() => api.addMod(device, server.name, path));
  };

  if (error && !value) return <div className="pane-body error-line">{error}</div>;
  return (
    <div className="pane-body">
      <section className="block">
        <div className="block-head">
          <span>Drop-in mods</span>
          {locked ? (
            <span className="row-line">Stop the server to change mods</span>
          ) : (
            <button className="button" disabled={busy} onClick={add}>
              <Plus />
              Add mod
            </button>
          )}
        </div>
        <div>
          {value?.map((m) => (
            <div key={m.file} className="mod-row">
              <div className="row-text">
                <div className="row-name">{m.id ?? m.file}</div>
                <div className="row-line mono">
                  {[m.version ? `v${m.version}` : null, bytes(m.bytes), m.file].filter(Boolean).join(" · ")}
                </div>
              </div>
              <span />
              <button
                className="icon-button"
                aria-label={`Remove ${m.id ?? m.file}`}
                disabled={busy || locked}
                onClick={() => change(() => api.removeMod(device, server.name, m.file))}
              >
                <Trash2 />
              </button>
            </div>
          ))}
          {value && value.length === 0 ? <p className="row-line">None</p> : null}
        </div>
      </section>
    </div>
  );
}

const COUNT = new Intl.NumberFormat();

function Pregen({
  device,
  server,
  onError,
}: {
  device: string;
  server: ServerInfo;
  onError: (err: unknown) => void;
}) {
  const up = server.state === "running";
  const { value, refresh } = usePoll(up ? () => api.pregen(device, server.name) : null, 2000, [
    device,
    server.name,
    up,
  ]);
  const [radius, setRadius] = useState("1000");
  const [centerX, setCenterX] = useState("0");
  const [centerZ, setCenterZ] = useState("0");
  const [circle, setCircle] = useState(false);
  const [busy, setBusy] = useState(false);

  const act = async (run: () => Promise<void>) => {
    setBusy(true);
    try {
      await run();
    } catch (err) {
      onError(err);
    } finally {
      setBusy(false);
      refresh();
    }
  };

  if (!up) return <div className="pane-body row-line">Start the server to pregenerate.</div>;
  const active = value?.state === "running" || value?.state === "paused";
  const r = Number(radius);
  const valid = Number.isInteger(r) && r > 0 && [centerX, centerZ].every((v) => Number.isInteger(Number(v)));

  return (
    <div className="pane-body">
      {active && value ? (
        <section className="block">
          <div className="block-head">
            <span>{value.state === "paused" ? "Paused" : "Generating"}</span>
            <div className="actions">
              <button
                className="button"
                disabled={busy}
                onClick={() =>
                  act(() =>
                    api.pregenAction(device, server.name, value.state === "paused" ? "continue" : "pause"),
                  )
                }
              >
                {value.state === "paused" ? <Play /> : <Pause />}
                {value.state === "paused" ? "Continue" : "Pause"}
              </button>
              <button
                className="button"
                disabled={busy}
                onClick={() => act(() => api.pregenAction(device, server.name, "cancel"))}
              >
                <Square />
                Cancel
              </button>
            </div>
          </div>
          <div className="stat-value">
            {(value.percent ?? 0).toFixed(1)}
            <small>%</small>
          </div>
          <div
            className="progress"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={value.percent ?? 0}
          >
            <span style={{ width: `${value.percent ?? 0}%` }} />
          </div>
          <p className="row-line">
            {[
              value.chunks != null ? `${COUNT.format(value.chunks)} chunks` : null,
              value.rate != null && value.state === "running" ? `${Math.round(value.rate)} per second` : null,
              value.eta_secs != null && value.state === "running" ? `${duration(value.eta_secs)} left` : null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
        </section>
      ) : (
        <section className="block">
          <form
            id="pregen-form"
            className="pregen-form"
            onSubmit={(e) => {
              e.preventDefault();
              if (!valid) return;
              act(() =>
                api.startPregen(device, server.name, {
                  radius: r,
                  center_x: Number(centerX),
                  center_z: Number(centerZ),
                  circle,
                }),
              );
            }}
          >
            <label className="field">
              <span>Radius</span>
              <input
                className="input"
                inputMode="numeric"
                value={radius}
                onChange={(e) => setRadius(e.target.value)}
              />
            </label>
            <label className="field">
              <span>Center X</span>
              <input
                className="input"
                inputMode="numeric"
                value={centerX}
                onChange={(e) => setCenterX(e.target.value)}
              />
            </label>
            <label className="field">
              <span>Center Z</span>
              <input
                className="input"
                inputMode="numeric"
                value={centerZ}
                onChange={(e) => setCenterZ(e.target.value)}
              />
            </label>
            <div className="field">
              <span>Shape</span>
              <div className="segmented" role="group" aria-label="Shape">
                <button type="button" aria-pressed={!circle} onClick={() => setCircle(false)}>
                  Square
                </button>
                <button type="button" aria-pressed={circle} onClick={() => setCircle(true)}>
                  Circle
                </button>
              </div>
            </div>
          </form>
          <div className="actions">
            <button className="button primary" type="submit" form="pregen-form" disabled={busy || !valid}>
              <Grid3x3 />
              Pregenerate
            </button>
            {value?.state === "finished" && value.chunks != null ? (
              <span className="row-line">Last run: {COUNT.format(value.chunks)} chunks</span>
            ) : null}
          </div>
        </section>
      )}
    </div>
  );
}

const WHEN = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

function Backups({
  device,
  server,
  onError,
}: {
  device: string;
  server: ServerInfo;
  onError: (err: unknown) => void;
}) {
  const { value, error, refresh } = usePoll(() => api.backups(device, server.name), 30_000, [
    device,
    server.name,
  ]);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<{ backup: Backup; action: "restore" | "remove" } | null>(null);
  const stopped = server.state === "stopped" || server.state === "crashed";

  const backUp = async () => {
    setBusy(true);
    try {
      await api.backUp(device, server.name);
    } catch (err) {
      onError(err);
    } finally {
      setBusy(false);
      refresh();
    }
  };

  if (error && !value) return <div className="pane-body error-line">{error}</div>;
  return (
    <div className="pane-body">
      <BackupSchedulePicker device={device} server={server.name} onError={onError} />
      <section className="block">
        <div className="block-head">
          <span>{value ? `${value.length} ${value.length === 1 ? "backup" : "backups"}` : null}</span>
          <button className="button" disabled={busy} onClick={backUp}>
            <Archive />
            {busy ? "Backing up…" : "Back up now"}
          </button>
        </div>
        <div>
          {value?.map((b) => (
            <div key={b.id} className="mod-row">
              <div className="row-text">
                <div className="row-name">{WHEN.format(b.created * 1000)}</div>
                <div className="row-line">
                  {b.scheduled ? "Automatic · " : null}
                  {b.files} files · {bytes(b.bytes)} · {bytes(b.stored)} new
                </div>
              </div>
              <button
                className="button"
                disabled={busy || !stopped}
                title={stopped ? undefined : "Stop the server to restore"}
                onClick={() => setConfirm({ backup: b, action: "restore" })}
              >
                <History />
                Restore
              </button>
              <button
                className="icon-button"
                aria-label="Remove backup"
                disabled={busy}
                onClick={() => setConfirm({ backup: b, action: "remove" })}
              >
                <Trash2 />
              </button>
            </div>
          ))}
          {value && value.length === 0 ? <p className="row-line">None yet</p> : null}
        </div>
      </section>
      <ConfirmDialog
        open={confirm !== null}
        title={
          confirm?.action === "restore"
            ? `Restore ${server.name} to ${WHEN.format(confirm.backup.created * 1000)}?`
            : "Remove this backup?"
        }
        body={
          confirm?.action === "restore"
            ? "Everything in the server folder goes back to how it was then. Changes since are lost unless you back up first."
            : "Chunks other backups share stay."
        }
        action={confirm?.action === "restore" ? "Restore" : "Remove"}
        onClose={() => setConfirm(null)}
        onConfirm={async () => {
          if (!confirm) return;
          try {
            if (confirm.action === "restore") await api.restoreBackup(device, server.name, confirm.backup.id);
            else await api.removeBackup(device, server.name, confirm.backup.id);
          } catch (err) {
            onError(err);
          } finally {
            refresh();
          }
        }}
      />
    </div>
  );
}

const SCHEDULES = [
  { label: "Off", every: 0, keep: 0 },
  { label: "Hourly", every: 60, keep: 24 },
  { label: "Every 6 hours", every: 360, keep: 8 },
  { label: "Daily", every: 1440, keep: 7 },
];

// Scheduled backups run while the server is running; retention only removes those, never ones taken by hand.
function BackupSchedulePicker({
  device,
  server,
  onError,
}: {
  device: string;
  server: string;
  onError: (err: unknown) => void;
}) {
  const [schedule, setSchedule] = useState<BackupSchedule | null | undefined>(undefined);
  const [keep, setKeep] = useState("");

  useEffect(() => {
    let live = true;
    api
      .backupSchedule(device, server)
      .then((s) => {
        if (!live) return;
        setSchedule(s);
        setKeep(s ? String(s.keep) : "");
      })
      .catch(onError);
    return () => {
      live = false;
    };
  }, [device, server]);

  const save = async (next: BackupSchedule | null) => {
    const before = schedule;
    setSchedule(next);
    setKeep(next ? String(next.keep) : "");
    try {
      await api.setBackupSchedule(device, server, next);
    } catch (err) {
      setSchedule(before);
      setKeep(before ? String(before.keep) : "");
      onError(err);
    }
  };

  const commitKeep = () => {
    const n = Number(keep);
    if (!schedule || !Number.isInteger(n) || n < 1 || n > 1000) {
      setKeep(schedule ? String(schedule.keep) : "");
      return;
    }
    if (n !== schedule.keep) save({ ...schedule, keep: n });
  };

  if (schedule === undefined) return null;
  return (
    <section className="block">
      <div className="block-head">
        <span>Automatic backups</span>
      </div>
      <div className="schedule-row">
        <div className="segmented" role="group" aria-label="Automatic backups">
          {SCHEDULES.map((s) => (
            <button
              key={s.every}
              type="button"
              aria-pressed={(schedule?.every_minutes ?? 0) === s.every}
              onClick={() => save(s.every === 0 ? null : { every_minutes: s.every, keep: s.keep })}
            >
              {s.label}
            </button>
          ))}
        </div>
        {schedule ? (
          <label className="field inline">
            <span>Keep</span>
            <input
              className="input"
              inputMode="numeric"
              value={keep}
              onChange={(e) => setKeep(e.target.value.replace(/\D/g, ""))}
              onBlur={commitKeep}
              onKeyDown={(e) => e.key === "Enter" && commitKeep()}
            />
          </label>
        ) : null}
      </div>
    </section>
  );
}

const SAMPLES = 120;

function Performance({ device, server }: { device: string; server: ServerInfo }) {
  const [samples, setSamples] = useState<Stats[]>([]);
  const { value, error } = usePoll(() => api.stats(device, server.name), 2000, [device, server.name]);

  useEffect(() => {
    if (!value) return;
    setSamples((prev) => [...prev.slice(-(SAMPLES - 1)), value]);
  }, [value]);

  if (error && !value) return <div className="pane-body error-line">{error}</div>;
  const latest = value;
  const memory = samples.map((s) => s.rss_bytes ?? 0);
  const cpu = samples.map((s) => s.cpu_percent ?? 0);
  const mspt = samples.map((s) => s.health?.mspt ?? 0);
  const health = latest?.health;

  return (
    <div className="pane-body">
      <div className="stats">
        {health ? (
          <>
            <div className="stat">
              <div className="stat-label">Tick time</div>
              <div className="stat-value">
                {health.mspt.toFixed(1)}
                <small>ms</small>
              </div>
              <Spark values={mspt} min={0} max={1000 / health.tickrate} />
            </div>
            <div className="stat">
              <div className="stat-label">TPS</div>
              <div className="stat-value">{health.tps.toFixed(1)}</div>
            </div>
            <div className="stat">
              <div className="stat-label">Players</div>
              <div className="stat-value">
                {health.players}
                <small>of {health.max_players}</small>
              </div>
            </div>
          </>
        ) : null}
        <div className="stat">
          <div className="stat-label">Memory</div>
          <div className="stat-value">{bytes(latest?.rss_bytes)}</div>
          <Spark values={memory} />
        </div>
        <div className="stat">
          <div className="stat-label">CPU</div>
          <div className="stat-value">
            {latest?.cpu_percent != null ? Math.round(latest.cpu_percent) : "–"}
            <small>%</small>
          </div>
          <Spark values={cpu} min={0} max={100} />
        </div>
        <div className="stat">
          <div className="stat-label">Uptime</div>
          <div className="stat-value">{duration(latest?.uptime_secs)}</div>
        </div>
      </div>
    </div>
  );
}

function Spark({ values, min, max }: { values: number[]; min?: number; max?: number }) {
  const lo = min ?? Math.min(...values) * 0.9;
  const hi = Math.max(max ?? 0, ...values) * (max ? 1 : 1.05);
  const span = hi - lo || 1;
  const points = values
    .map((v, i) => `${(i / (SAMPLES - 1)) * 100},${40 - ((v - lo) / span) * 38 - 1}`)
    .join(" ");
  return (
    <svg className="spark" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden>
      <line x1="0" y1="39.5" x2="100" y2="39.5" />
      {values.length > 1 ? <polyline points={points} /> : null}
    </svg>
  );
}
