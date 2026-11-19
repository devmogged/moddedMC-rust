import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Archive, Grid3x3, History, Pause, Play, Plus, RotateCw, Square, Trash2, Upload } from "lucide-react";
import {
  api,
  minecraftAddress,
  type Backup,
  type BackupSchedule,
  type Device,
  type ServerInfo,
  type Stats,
} from "./api";
import {
  LoadingRows,
  LoadError,
  ServerCluster,
  ServerIcon,
  bytes,
  duration,
  healthBars,
  packLine,
  usePoll,
  useServerIcon,
} from "./ui";
import { HistoryPlot, PregenMap } from "./Metrics";
import { ConfirmDialog, DeployDialog } from "./dialogs";
import type { PregenRead } from "./App";
import { isPregenActive, type Lifecycle } from "./serverCommands";

export type Tab = "console" | "performance" | "mods" | "backups" | "pregen";

export const TABS: { id: Tab; label: string }[] = [
  { id: "console", label: "Console" },
  { id: "performance", label: "Performance" },
  { id: "mods", label: "Mods" },
  { id: "backups", label: "Backups" },
  { id: "pregen", label: "Pregen" },
];

export function ServerPane({
  device,
  server,
  tab,
  onTab,
  onAct,
  backupBusy,
  backupRevision,
  onBackup,
  pregen,
  onPregenChanged,
  onDeployed,
  onError,
}: {
  device: Device;
  server: ServerInfo;
  tab: Tab;
  onTab: (tab: Tab) => void;
  onAct: (action: Lifecycle) => void;
  backupBusy: boolean;
  backupRevision: number;
  onBackup: () => void;
  pregen: PregenRead | undefined;
  onPregenChanged: (started?: boolean) => void;
  onDeployed: () => void;
  onError: (err: unknown) => void;
}) {
  const tabList = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const list = tabList.current;
    if (!list) return;
    const move = () => {
      const selected = list.querySelector<HTMLButtonElement>('[aria-selected="true"]');
      if (!selected) return;
      list.style.setProperty("--tab-x", selected.offsetLeft + "px");
      list.style.setProperty("--tab-width", String(selected.offsetWidth));
    };
    move();
    const observer = new ResizeObserver(move);
    observer.observe(list);
    return () => observer.disconnect();
  }, [tab, server.external]);

  const [deploying, setDeploying] = useState(false);
  const showStats = !server.external && (server.state === "running" || tab === "performance");
  const {
    value: latest,
    error: statsError,
    refresh: retryStats,
  } = usePoll(showStats ? () => api.stats(device.id, server.name) : null, 2000, [
    device.id,
    server.name,
    showStats,
  ]);
  const [samples, setSamples] = useState<Stats[]>([]);
  useEffect(() => {
    if (latest) setSamples((prev) => [...prev.slice(-119), latest]);
  }, [latest]);
  const telemetry = server.state === "running" ? (latest?.health ?? server.health) : null;
  const tickTone = telemetry ? healthBars(telemetry).tone : "unknown";

  const icon = useServerIcon(device, server);
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
    <div className="pane server-identity" style={icon.style}>
      <header className="pane-head server-heading">
        <div className="row large">
          <ServerIcon name={server.name} favicon={icon.favicon} />
          <div className="row-text">
            <h1 className="row-name">{server.name}</h1>
            <p className="row-line">{line}</p>
          </div>
          <ServerCluster device={device} server={server} detail />
        </div>
        <div className="actions">
          {server.external ? null : up || moving ? (
            <>
              <button
                className="button"
                disabled={moving}
                title={moving ? "Server is changing state" : undefined}
                onClick={() => onAct("restart")}
              >
                <RotateCw />
                Restart
              </button>
              <button
                className="button"
                disabled={moving}
                title={moving ? "Server is changing state" : undefined}
                onClick={() => onAct("stop")}
              >
                <Square />
                Stop
              </button>
            </>
          ) : (
            <>
              <button className="button" onClick={() => setDeploying(true)}>
                <Upload />
                Deploy
              </button>
              <button className="button primary" onClick={() => onAct("start")}>
                <Play />
                Start
              </button>
            </>
          )}
        </div>
      </header>

      {!server.external && (
        <div className="live-strip">
          <LiveReading
            label="TPS"
            value={telemetry?.tps.toFixed(1)}
            values={samples.map((s) => s.health?.tps ?? null)}
            max={telemetry?.tickrate}
            tone={tickTone}
          />
          <LiveReading
            label="MSPT"
            value={telemetry?.mspt.toFixed(1)}
            values={samples.map((s) => s.health?.mspt ?? null)}
            max={telemetry ? 1000 / telemetry.tickrate : undefined}
            tone={tickTone}
          />
          <LiveReading
            label="Players"
            value={telemetry ? telemetry.players + "/" + telemetry.max_players : undefined}
            values={samples.map((s) => s.health?.players ?? null)}
            max={telemetry?.max_players}
            tone="starting"
          />
          <LiveReading
            label="RAM"
            value={up && latest?.rss_bytes != null ? bytes(latest.rss_bytes) : undefined}
            values={samples.map((s) => s.rss_bytes)}
            tone="external"
          />
        </div>
      )}

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
          <div className="tabs" role="tablist" aria-label="Server" ref={tabList}>
            <span className="tab-slide" aria-hidden />
            {TABS.map((t) => (
              <button
                key={t.id}
                className="tab"
                role="tab"
                id={"tab-" + t.id}
                aria-controls="server-panel"
                aria-selected={tab === t.id}
                tabIndex={tab === t.id ? 0 : -1}
                onKeyDown={(event) => {
                  const current = TABS.findIndex(({ id }) => id === t.id);
                  const next =
                    event.key === "Home"
                      ? 0
                      : event.key === "End"
                        ? TABS.length - 1
                        : event.key === "ArrowLeft"
                          ? (current - 1 + TABS.length) % TABS.length
                          : event.key === "ArrowRight"
                            ? (current + 1) % TABS.length
                            : null;
                  if (next == null) return;
                  event.preventDefault();
                  onTab(TABS[next].id);
                  document.getElementById("tab-" + TABS[next].id)?.focus();
                }}
                onClick={() => onTab(t.id)}
              >
                {t.label}
              </button>
            ))}
          </div>
          <div
            key={tab}
            className="tab-panel"
            role="tabpanel"
            id="server-panel"
            aria-labelledby={"tab-" + tab}
            tabIndex={0}
          >
            {tab === "console" ? (
              <Console device={device.id} server={server.name} />
            ) : tab === "performance" ? (
              <Performance samples={samples} latest={latest} error={statsError} retry={retryStats} />
            ) : tab === "mods" ? (
              <Mods device={device.id} server={server} onError={onError} />
            ) : tab === "backups" ? (
              <Backups
                device={device.id}
                server={server}
                busy={backupBusy}
                revision={backupRevision}
                onBackup={onBackup}
                onError={onError}
              />
            ) : (
              <Pregen
                device={device.id}
                server={server}
                snapshot={pregen}
                onChanged={onPregenChanged}
                onError={onError}
              />
            )}
          </div>
        </>
      )}

      <DeployDialog
        open={deploying}
        device={device}
        server={server.name}
        onClose={() => setDeploying(false)}
        onDeployed={onDeployed}
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
  const lastScroll = useRef(0);
  const firstLines = useRef(true);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    let alive = true;
    let retry: number | undefined;
    const unlisten = api.onConsole(({ id, line }) => {
      if (id !== idRef.current) return;
      if (line === null) {
        idRef.current = null;
        setConnected(false);
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
        setConnected(true);
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
    if (!log || !follow.current || !lines.length) return;
    const instant = firstLines.current || matchMedia("(prefers-reduced-motion: reduce)").matches;
    log.scrollTo({ top: log.scrollHeight, behavior: instant ? "instant" : "smooth" });
    firstLines.current = false;
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
        aria-label="Console output"
        tabIndex={0}
        onWheel={(event) => {
          if (event.deltaY >= 0) return;
          follow.current = false;
          const log = event.currentTarget;
          log.scrollTo({ top: log.scrollTop, behavior: "instant" });
        }}
        onScroll={(e) => {
          const el = e.currentTarget;
          if (el.scrollTop < lastScroll.current) follow.current = false;
          if (el.scrollHeight - el.scrollTop - el.clientHeight < 2) follow.current = true;
          lastScroll.current = el.scrollTop;
        }}
      >
        {!connected && !error && lines.length === 0 && <LoadingRows />}
        {lines.map((line, i) => (
          <div key={i} data-level={level(line)}>
            {line.startsWith("[") && line.indexOf("]") > 0 ? (
              <>
                <span className="log-time">{line.slice(0, line.indexOf("]") + 1)}</span>
                {line.slice(line.indexOf("]") + 1)}
              </>
            ) : (
              line
            )}
          </div>
        ))}
        {error ? <div data-level="error">{error}</div> : null}
      </div>
      <label className="console-input">
        <span aria-hidden>&gt;</span>
        <input
          value={input}
          disabled={!connected}
          title={connected ? undefined : "Waiting for the console connection"}
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

  if (error && !value)
    return (
      <div className="pane-body">
        <LoadError error={error} retry={refresh} />
      </div>
    );
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
          {!value && <LoadingRows />}
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
                title={locked ? "Stop the server to change mods" : undefined}
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
  snapshot,
  onChanged,
  onError,
}: {
  device: string;
  server: ServerInfo;
  snapshot: PregenRead | undefined;
  onChanged: (started?: boolean) => void;
  onError: (err: unknown) => void;
}) {
  const up = server.state === "running";
  const { value, error } = snapshot ?? {};
  const [radius, setRadius] = useState("1000");
  const [centerX, setCenterX] = useState("0");
  const [centerZ, setCenterZ] = useState("0");
  const [circle, setCircle] = useState(false);
  const [busy, setBusy] = useState(false);

  const act = async (run: () => Promise<void>, startsTask = false) => {
    setBusy(true);
    try {
      await run();
      onChanged(startsTask);
    } catch (err) {
      onError(err);
      onChanged();
    } finally {
      setBusy(false);
    }
  };

  if (!up) return <div className="pane-body row-line">Start the server to pregenerate.</div>;
  if (error && !value)
    return (
      <div className="pane-body">
        <LoadError error={error} retry={() => onChanged()} />
      </div>
    );
  if (!value)
    return (
      <div className="pane-body">
        <LoadingRows />
      </div>
    );
  const active = isPregenActive(value.state);
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
          <div className="pregen-readout" data-tone={value.state === "paused" ? "warn" : "ok"}>
            <PregenMap percent={value.percent} />
            <div>
              {value.world && <p className="row-line">{value.world}</p>}
              <div className="stat-value">
                {value.percent == null ? "–" : value.percent.toFixed(1)}
                <small>%</small>
              </div>
              <p className="row-line">
                {[
                  value.chunks != null ? `${COUNT.format(value.chunks)} chunks` : null,
                  value.rate != null && value.state === "running"
                    ? `${Math.round(value.rate)} per second`
                    : null,
                  value.eta_secs != null && value.state === "running"
                    ? `${duration(value.eta_secs)} left`
                    : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </p>
            </div>
          </div>
        </section>
      ) : (
        <section className="block">
          <form
            id="pregen-form"
            className="pregen-form"
            onSubmit={(e) => {
              e.preventDefault();
              if (!valid) return;
              act(
                () =>
                  api.startPregen(device, server.name, {
                    radius: r,
                    center_x: Number(centerX),
                    center_z: Number(centerZ),
                    circle,
                  }),
                true,
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
  busy: backingUp,
  revision,
  onBackup,
  onError,
}: {
  device: string;
  server: ServerInfo;
  busy: boolean;
  revision: number;
  onBackup: () => void;
  onError: (err: unknown) => void;
}) {
  const { value, error, refresh } = usePoll(() => api.backups(device, server.name), 30_000, [
    device,
    server.name,
    revision,
  ]);
  const [mutating, setMutating] = useState(false);
  const busy = mutating || backingUp;
  const [confirm, setConfirm] = useState<{ backup: Backup; action: "restore" | "remove" } | null>(null);
  const stopped = server.state === "stopped" || server.state === "crashed";

  if (error && !value)
    return (
      <div className="pane-body">
        <LoadError error={error} retry={refresh} />
      </div>
    );
  return (
    <div className="pane-body">
      <BackupSchedulePicker device={device} server={server.name} onError={onError} />
      <section className="block">
        <div className="block-head">
          <span>{value ? `${value.length} ${value.length === 1 ? "backup" : "backups"}` : null}</span>
          <button className="button" disabled={busy} onClick={onBackup}>
            <Archive />
            {backingUp ? "Backing up…" : "Back up now"}
          </button>
        </div>
        <div>
          {!value && <LoadingRows />}
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
          if (!confirm || busy) return;
          setMutating(true);
          try {
            if (confirm.action === "restore") await api.restoreBackup(device, server.name, confirm.backup.id);
            else await api.removeBackup(device, server.name, confirm.backup.id);
          } catch (err) {
            onError(err);
          } finally {
            setMutating(false);
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

function LiveReading({
  label,
  value,
  values,
  max,
  tone,
}: {
  label: string;
  value?: string;
  values: (number | null)[];
  max?: number;
  tone: string;
}) {
  return (
    <div className="live-reading" data-tone={value == null ? "unknown" : tone}>
      <span>{label}</span>
      <RollingValue value={value ?? "–"} />
      <HistoryPlot values={value == null ? [] : values.slice(-12)} max={max} tone={tone} />
    </div>
  );
}

function RollingValue({ value }: { value: string }) {
  const node = useRef<HTMLSpanElement>(null);
  const previous = useRef(value);
  useLayoutEffect(() => {
    const from = previous.current;
    previous.current = value;
    if (
      !node.current ||
      from === value ||
      from === "–" ||
      value === "–" ||
      matchMedia("(prefers-reduced-motion: reduce)").matches
    )
      return;
    const animation = node.current.animate(
      [
        { transform: "translateY(62%)", opacity: 0 },
        { transform: "translateY(0)", opacity: 1 },
      ],
      { duration: 185, easing: "cubic-bezier(.18, .78, .28, 1)" },
    );
    return () => animation.cancel();
  }, [value]);
  return (
    <strong className="rolling-value">
      <span ref={node}>{value}</span>
    </strong>
  );
}

function Performance({
  latest,
  samples,
  error,
  retry,
}: {
  latest: Stats | null;
  samples: Stats[];
  error: string | null;
  retry: () => void;
}) {
  const health = latest?.health;
  const tick = samples.map((s) => s.health?.mspt ?? null);
  const cpu = samples.map((s) => s.cpu_percent);
  const ram = samples.map((s) => s.rss_bytes);
  const readings = ram.filter((value): value is number => value !== null);
  return (
    <div className="pane-body">
      {error && <LoadError error={error} retry={retry} />}
      {!latest && !error && <LoadingRows />}
      <div className="performance-grid">
        {(health || tick.some((v) => v != null)) && (
          <section className="chart-panel tick-chart">
            <div className="chart-head">
              <div>
                <h2>Tick time</h2>
                <div className="stat-value">
                  {health?.mspt.toFixed(1) ?? "–"}
                  <small>ms</small>
                </div>
              </div>
              <div className="chart-summary">
                <span>
                  TPS <strong>{health?.tps.toFixed(1) ?? "–"}</strong>
                </span>
                <span>
                  Players <strong>{health ? health.players + " of " + health.max_players : "–"}</strong>
                </span>
              </div>
            </div>
            <HistoryPlot
              values={tick}
              max={health ? 1000 / health.tickrate : undefined}
              tone={health ? healthBars(health).tone : "unknown"}
              label="Tick time history"
              format={(n) => n.toFixed(1) + " ms"}
            />
          </section>
        )}
        <section className="chart-panel">
          <div className="chart-head">
            <div>
              <h2>Memory</h2>
              <div className="stat-value">{bytes(latest?.rss_bytes)}</div>
            </div>
          </div>
          <HistoryPlot
            values={ram}
            min={readings.length ? Math.min(...readings) * 0.9 : undefined}
            tone="external"
            label="Memory history"
            format={bytes}
          />
        </section>
        <section className="chart-panel">
          <div className="chart-head">
            <div>
              <h2>CPU</h2>
              <div className="stat-value">
                {latest?.cpu_percent != null ? Math.round(latest.cpu_percent) : "–"}
                <small>%</small>
              </div>
            </div>
          </div>
          <HistoryPlot
            values={cpu}
            max={100}
            tone="starting"
            label="CPU history"
            format={(n) => n.toFixed(1) + "%"}
          />
        </section>
      </div>
      <dl className="facts performance-uptime">
        <dt>Uptime</dt>
        <dd>{duration(latest?.uptime_secs)}</dd>
      </dl>
    </div>
  );
}
