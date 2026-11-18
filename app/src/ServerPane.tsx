import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Play, Plus, RotateCw, Square, Trash2, Upload } from "lucide-react";
import { api, minecraftAddress, type Device, type ServerInfo, type Stats } from "./api";
import { ServerCluster, ServerIcon, bytes, duration, packLine, usePoll } from "./ui";
import { DeployDialog } from "./dialogs";

type Tab = "console" | "performance" | "mods";

const TABS: { id: Tab; label: string }[] = [
  { id: "console", label: "Console" },
  { id: "performance", label: "Performance" },
  { id: "mods", label: "Mods" },
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
          ) : (
            <Mods device={device.id} server={server} onError={onError} />
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
