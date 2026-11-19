import { useCallback, useEffect, useRef, useState } from "react";
import {
  Archive,
  Boxes,
  Check,
  CircleAlert,
  HardDrive,
  Package,
  Play,
  Plus,
  RotateCw,
  Search,
  Server,
  Square,
  X,
} from "lucide-react";
import { api, type Device, type Friend, type Pregen, type ServerInfo } from "./api";
import { LoadingRows, LoadError, HostedServerRow, routeLabel, usePoll } from "./ui";
import { DeviceMeters } from "./Metrics";
import { ServerPane, TABS, type Tab } from "./ServerPane";
import { DevicePane } from "./DevicePane";
import { FriendPane, FriendRow, useFriendStatus } from "./FriendPane";
import { ModsPane } from "./ModsPane";
import { PackPane } from "./PackPane";
import { CommandPalette, type Command } from "./CommandPalette";
import {
  commandReason,
  isPregenActive,
  serverKey,
  type Lifecycle,
  type ServerCommand,
} from "./serverCommands";
import { AddDeviceDialog, AddFriendDialog } from "./dialogs";

export type Selection =
  | { kind: "server"; device: string; server: string; tab?: Tab }
  | { kind: "device"; device: string }
  | { kind: "friend"; id: string }
  | { kind: "pack"; path: string }
  | { kind: "mods" };

export interface DeviceData {
  servers: ServerInfo[] | null;
  error: string | null;
}

const SELECTION_KEY = "selection";

function loadSelection(): Selection {
  // Dev previews open a pane directly: #server/pi/survival, #device/pi, #friend/sting, #mods, #pack/<pack.toml path>.
  if (import.meta.env.DEV && location.hash) {
    const [kind, a, b, tab] = location.hash.slice(1).split("/");
    if (kind === "server" && a && b)
      return { kind, device: a, server: b, tab: TABS.find(({ id }) => id === tab)?.id };
    if (kind === "device" && a) return { kind, device: a };
    if (kind === "friend" && a) return { kind, id: a };
    if (kind === "mods") return { kind };
    if (kind === "pack" && a) return { kind, path: location.hash.slice("#pack/".length) };
  }
  try {
    const raw = localStorage.getItem(SELECTION_KEY);
    if (raw) return JSON.parse(raw) as Selection;
  } catch {
    // Storage may be unavailable; start on this PC.
  }
  return { kind: "device", device: "local" };
}

/** Polls every device's server list together, so the sidebar and panes share one view. */
function useDeviceData(devices: Device[]) {
  const [data, setData] = useState<Record<string, DeviceData>>({});
  const [tick, setTick] = useState(0);
  const [overrides, setOverrides] = useState<Record<string, "starting" | "stopping">>({});
  const actions = useRef(new Map<string, boolean>());
  const refresh = useCallback(() => setTick((t) => t + 1), []);
  const act = async (device: string, server: ServerInfo, action: Lifecycle) => {
    const key = serverKey(device, server.name);
    if (actions.current.has(key) || commandReason(server, action, false)) return;
    actions.current.set(key, false);
    setOverrides((prev) => ({ ...prev, [key]: action === "stop" ? "stopping" : "starting" }));
    try {
      await api.action(device, server.name, action);
      actions.current.set(key, true);
    } catch (err) {
      actions.current.delete(key);
      setOverrides((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
      throw err;
    } finally {
      refresh();
    }
  };
  const ids = devices.map((d) => d.id).join("|");
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const run = async () => {
      const settled = [...actions.current].filter(([, completed]) => completed).map(([key]) => key);
      const results = await Promise.all(
        devices.map(async (device) => {
          try {
            return [device.id, { servers: await api.servers(device.id), error: null }] as const;
          } catch (err) {
            return [device.id, { servers: null, error: (err as Error).message }] as const;
          }
        }),
      );
      if (!alive) return;
      setData(Object.fromEntries(results));
      if (settled.length) {
        for (const key of settled) actions.current.delete(key);
        setOverrides((prev) => {
          const next = { ...prev };
          for (const key of settled) delete next[key];
          return next;
        });
      }
      timer = window.setTimeout(run, 2000);
    };
    run();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ids, tick]);
  const view = Object.fromEntries(
    Object.entries(data).map(([id, result]) => [
      id,
      {
        ...result,
        servers:
          result.servers?.map((server) => {
            const state = overrides[serverKey(id, server.name)];
            return state ? { ...server, state } : server;
          }) ?? null,
      },
    ]),
  );
  return { data: view, refresh, act };
}

export function App() {
  const devicesPoll = usePoll(api.devices, 60_000, []);
  const friendsPoll = usePoll(api.friends, 60_000, []);
  // Packs are files on this machine; reread them so a build or an edit shows up.
  const packsPoll = usePoll(api.packs, 5_000, []);
  const packs = packsPoll.value ?? [];
  const devices = devicesPoll.value ?? [];
  const friends = friendsPoll.value ?? [];
  const { data, refresh, act } = useDeviceData(devices);
  const [selection, setSelectionState] = useState<Selection>(loadSelection);
  const [dialog, setDialog] = useState<"device" | "friend" | null>(null);
  const [palette, setPalette] = useState(false);
  const [toasts, setToasts] = useState<Notice[]>([]);
  const toastId = useRef(0);
  const [backupState, setBackupState] = useState<Record<string, { busy: boolean; revision: number }>>({});
  const backupLocks = useRef(new Set<string>());
  const [pregen, setPregen] = useState<Record<string, PregenRead>>({});
  const previousPregen = useRef<Record<string, Pregen["state"]>>({});
  const [pregenRevision, setPregenRevision] = useState(0);
  const [pregenRequested, setPregenRequested] = useState<Record<string, ServerTarget>>({});
  const notice = useCallback((message: string, tone: Notice["tone"] = "ok") => {
    const id = ++toastId.current;
    setToasts((prev) => [...prev.slice(-3), { message, tone, id }]);
  }, []);
  const dismiss = useCallback((id: number) => {
    setToasts((prev) => prev.filter((item) => item.id !== id));
  }, []);
  const reportPregen = useCallback(
    (key: string, snapshot: PregenRead, target: ServerTarget) => {
      if (snapshot.value) {
        if (snapshot.value.state === "finished" && isPregenActive(previousPregen.current[key])) {
          notice("Pregen finished · " + target.server);
        }
        previousPregen.current[key] = snapshot.value.state;
      }
      setPregen((prev) => ({ ...prev, [key]: snapshot }));
      if (isPregenActive(snapshot.value?.state)) {
        setPregenRequested((prev) => (key in prev ? prev : { ...prev, [key]: target }));
      } else if (snapshot.value) {
        setPregenRequested((prev) => {
          if (!(key in prev)) return prev;
          const next = { ...prev };
          delete next[key];
          return next;
        });
      }
    },
    [notice],
  );
  const focusWorkspace = () =>
    requestAnimationFrame(() => {
      const main = document.querySelector<HTMLElement>("main");
      (main?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]') ?? main)?.focus();
    });

  const select = (next: Selection) => {
    setSelectionState(next);
    try {
      localStorage.setItem(SELECTION_KEY, JSON.stringify(next));
    } catch {
      // Not remembered across launches; fine.
    }
  };

  useEffect(() => {
    const open = (event: KeyboardEvent) => {
      if (
        event.key.toLowerCase() !== "k" ||
        !(event.ctrlKey || event.metaKey) ||
        event.altKey ||
        event.repeat
      )
        return;
      if (document.querySelector("dialog[open]:not(.palette)")) return;
      event.preventDefault();
      setPalette((current) => !current);
    };
    window.addEventListener("keydown", open);
    return () => window.removeEventListener("keydown", open);
  }, []);

  const addPack = async () => {
    try {
      const path = await api.pickPackFile();
      if (!path) return;
      const pack = await api.addPack(path);
      packsPoll.refresh();
      select({ kind: "pack", path: pack.path });
    } catch (err) {
      fail(err);
    }
  };

  const fail = useCallback(
    (err: unknown) => {
      notice(err instanceof Error ? err.message : String(err), "bad");
    },
    [notice],
  );

  const device = (id: string) => devices.find((d) => d.id === id);

  const run = async (device: string, server: ServerInfo, action: ServerCommand) => {
    const key = serverKey(device, server.name);
    if (commandReason(server, action, backupLocks.current.has(key))) return;
    if (action !== "backup") {
      try {
        await act(device, server, action);
      } catch (err) {
        fail(err);
      }
      return;
    }
    backupLocks.current.add(key);
    setBackupState((prev) => ({ ...prev, [key]: { busy: true, revision: prev[key]?.revision ?? 0 } }));
    let succeeded = false;
    try {
      await api.backUp(device, server.name);
      succeeded = true;
      notice("Backup complete · " + server.name);
    } catch (err) {
      fail(err);
    } finally {
      backupLocks.current.delete(key);
      setBackupState((prev) => ({
        ...prev,
        [key]: { busy: false, revision: (prev[key]?.revision ?? 0) + Number(succeeded) },
      }));
    }
  };

  const commands: Command[] = [];
  const jump = (id: string, label: string, icon: Command["icon"], target: Selection, context?: string) => {
    commands.push({ id, label, icon, context, run: () => select(target) });
  };
  const watched = new Map<string, ServerTarget>();
  for (const d of devices) {
    jump("device:" + d.id, d.name, HardDrive, { kind: "device", device: d.id }, routeLabel[d.route]);
    for (const s of data[d.id]?.servers ?? []) {
      const key = serverKey(d.id, s.name);
      const target = { kind: "server", device: d.id, server: s.name } as const;
      const context = s.name + " · " + d.name;
      jump(key, s.name, Server, target, d.name);
      if (s.external) continue;
      const visible =
        selection.kind === "server" &&
        selection.device === d.id &&
        selection.server === s.name &&
        selection.tab === "pregen";
      if (
        s.state === "running" &&
        (visible || isPregenActive(pregen[key]?.value?.state) || pregenRequested[key])
      ) {
        watched.set(key, { device: d.id, server: s.name });
      }
      for (const tab of TABS)
        jump(key + "/" + tab.id, tab.label, Server, { ...target, tab: tab.id }, context);
      for (const command of SERVER_ACTIONS) {
        commands.push({
          ...command,
          id: key + ":" + command.action,
          context,
          reason: commandReason(s, command.action, backupState[key]?.busy ?? false),
          run: () => {
            select(command.action === "backup" ? { ...target, tab: "backups" } : target);
            void run(d.id, s, command.action);
          },
        });
      }
    }
  }
  // Keep an observed task after navigation, including across a temporary device outage.
  for (const [key, target] of Object.entries(pregenRequested)) {
    if (
      !data[target.device]?.servers ||
      data[target.device].servers?.some(
        (server) => server.name === target.server && server.state === "running",
      )
    ) {
      watched.set(key, target);
    }
  }
  for (const pack of packs)
    jump("pack:" + pack.path, pack.name, Package, { kind: "pack", path: pack.path }, "Packs");
  for (const friend of friends)
    jump("friend:" + friend.id, friend.name, Server, { kind: "friend", id: friend.id }, "Join");
  jump("mods", "Mods", Boxes, { kind: "mods" });

  let pane = null;
  const server =
    selection.kind === "server"
      ? data[selection.device]?.servers?.find((s) => s.name === selection.server)
      : undefined;
  if (selection.kind === "server" && server && device(selection.device)) {
    const d = device(selection.device) as Device;
    const key = serverKey(d.id, server.name);
    pane = (
      <ServerPane
        key={`${d.id}/${server.name}`}
        device={d}
        server={server}
        tab={selection.tab ?? "console"}
        onTab={(tab) => select({ ...selection, tab })}
        onAct={(action) => run(d.id, server, action)}
        backupBusy={backupState[key]?.busy ?? false}
        backupRevision={backupState[key]?.revision ?? 0}
        onBackup={() => run(d.id, server, "backup")}
        pregen={pregen[key]}
        onPregenChanged={(started) => {
          if (started) {
            previousPregen.current[key] = "running";
            setPregenRequested((prev) => ({ ...prev, [key]: { device: d.id, server: server.name } }));
            setPregen((prev) => ({
              ...prev,
              [key]: {
                value: {
                  state: "running",
                  world: null,
                  chunks: null,
                  percent: null,
                  eta_secs: null,
                  rate: null,
                },
                error: null,
              },
            }));
          }
          setPregenRevision((version) => version + 1);
        }}
        onDeployed={() => {
          notice("Deploy complete · " + server.name);
          refresh();
        }}
        onError={fail}
      />
    );
  } else if (selection.kind === "server" || selection.kind === "device") {
    // A server that is gone (or not listed yet) falls back to its device.
    const d = device(selection.device) ?? device("local");
    if (d && (selection.kind === "device" || data[d.id])) {
      pane = (
        <DevicePane
          key={d.id}
          device={d}
          data={data[d.id]}
          onSelect={select}
          onChanged={refresh}
          onRemoved={() => {
            devicesPoll.refresh();
            select({ kind: "device", device: "local" });
          }}
          onError={fail}
        />
      );
    }
  } else if (selection.kind === "friend") {
    const friend = friends.find((f) => f.id === selection.id);
    if (friend) {
      pane = (
        <FriendPane
          key={friend.id}
          friend={friend}
          onRemoved={() => {
            friendsPoll.refresh();
            select({ kind: "device", device: "local" });
          }}
          onError={fail}
        />
      );
    }
  } else if (selection.kind === "pack") {
    const pack = packs.find((p) => p.path === selection.path);
    if (pack) {
      pane = (
        <PackPane
          key={pack.path}
          pack={pack}
          onDeployed={(server) => notice("Deploy complete · " + server)}
          devices={devices}
          data={data}
          onChanged={() => {
            packsPoll.refresh();
            refresh();
          }}
          onRemoved={() => {
            packsPoll.refresh();
            select({ kind: "device", device: "local" });
          }}
          onError={fail}
        />
      );
    }
  } else {
    pane = <ModsPane key="mods" />;
  }

  const pending =
    selection.kind === "device" || selection.kind === "server"
      ? devicesPoll
      : selection.kind === "friend"
        ? friendsPoll
        : packsPoll;
  if (!pane)
    pane = (
      <div className="pane">
        <div className="pane-body">
          {pending.error ? (
            <LoadError error={pending.error} retry={pending.refresh} />
          ) : pending.value && (selection.kind === "friend" || selection.kind === "pack") ? (
            <LoadError
              error="This item is no longer available."
              retry={() => select({ kind: "device", device: "local" })}
            />
          ) : (
            <LoadingRows />
          )}
        </div>
      </div>
    );

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <Mark />
          moddedMC
          <button
            className="command-trigger"
            aria-label="Open command palette"
            aria-keyshortcuts="Control+K Meta+K"
            onClick={() => setPalette(true)}
          >
            <Search aria-hidden />
            <kbd>{navigator.platform.includes("Mac") ? "⌘ K" : "Ctrl K"}</kbd>
          </button>
        </div>
        <div className="sidebar-scroll">
          <section className="section" aria-label="Devices">
            <div className="section-head">
              Devices
              <button
                className="icon-button"
                aria-label="Add device"
                title="Add device"
                onClick={() => setDialog("device")}
              >
                <Plus />
              </button>
            </div>
            {!devicesPoll.value && !devicesPoll.error && <LoadingRows />}
            {devices.map((d) => (
              <DeviceGroup key={d.id} device={d} data={data[d.id]} selection={selection} onSelect={select} />
            ))}
          </section>

          <section className="section" aria-label="Join">
            <div className="section-head">
              Join
              <button
                className="icon-button"
                aria-label="Add server"
                title="Add server"
                onClick={() => setDialog("friend")}
              >
                <Plus />
              </button>
            </div>
            {friends.map((f) => (
              <SidebarFriend
                key={f.id}
                friend={f}
                current={selection.kind === "friend" && selection.id === f.id}
                onSelect={() => select({ kind: "friend", id: f.id })}
              />
            ))}
          </section>

          <section className="section" aria-label="Packs">
            <div className="section-head">
              Packs
              <button className="icon-button" aria-label="Add pack" title="Add pack" onClick={addPack}>
                <Plus />
              </button>
            </div>
            {packs.map((p) => (
              <button
                key={p.path}
                className="nav-item"
                aria-current={selection.kind === "pack" && selection.path === p.path}
                onClick={() => select({ kind: "pack", path: p.path })}
              >
                <Package />
                <span className="nav-label">{p.name}</span>
                <span className="nav-meta">{p.error ? "Error" : p.version ? `v${p.version}` : ""}</span>
              </button>
            ))}
          </section>
        </div>
        <section className="sidebar-footer" aria-label="Mods">
          <button
            className="nav-item"
            aria-current={selection.kind === "mods"}
            onClick={() => select({ kind: "mods" })}
          >
            <Boxes />
            Mods
          </button>
        </section>
      </aside>

      <main className="main" tabIndex={-1} aria-label="Workspace">
        {pane}
      </main>

      <AddDeviceDialog
        open={dialog === "device"}
        onClose={() => setDialog(null)}
        onAdded={(d) => {
          devicesPoll.refresh();
          select({ kind: "device", device: d.id });
        }}
      />
      <AddFriendDialog
        open={dialog === "friend"}
        onClose={() => setDialog(null)}
        onAdded={(f) => {
          friendsPoll.refresh();
          select({ kind: "friend", id: f.id });
        }}
      />

      <CommandPalette
        open={palette}
        commands={commands}
        close={() => setPalette(false)}
        onRun={(command) => {
          setPalette(false);
          command.run();
          focusWorkspace();
        }}
      />
      {[...watched].map(([key, target]) => (
        <PregenWatch key={key} id={key} target={target} revision={pregenRevision} onReport={reportPregen} />
      ))}
      <div className="toast-stack">
        {toasts.map((item) => (
          <Toast key={item.id} item={item} dismiss={dismiss} />
        ))}
      </div>
    </div>
  );
}

const SERVER_ACTIONS = [
  { label: "Start", icon: Play, action: "start" },
  { label: "Stop", icon: Square, action: "stop" },
  { label: "Restart", icon: RotateCw, action: "restart" },
  { label: "Back up now", icon: Archive, action: "backup" },
] satisfies { label: string; icon: Command["icon"]; action: ServerCommand }[];

type Notice = { message: string; tone: "ok" | "bad"; id: number };
type ServerTarget = { device: string; server: string };
export type PregenRead = { value: Pregen | null; error: string | null };

function Toast({ item, dismiss }: { item: Notice; dismiss: (id: number) => void }) {
  useEffect(() => {
    const timer = window.setTimeout(() => dismiss(item.id), item.tone === "bad" ? 9000 : 5500);
    return () => window.clearTimeout(timer);
  }, [dismiss, item]);
  const Icon = item.tone === "ok" ? Check : CircleAlert;
  return (
    <div className="toast" data-tone={item.tone}>
      <Icon aria-hidden />
      <span role="status">{item.message}</span>
      <button className="icon-button" onClick={() => dismiss(item.id)} aria-label="Dismiss notification">
        <X />
      </button>
    </div>
  );
}

function PregenWatch({
  id,
  target,
  revision,
  onReport,
}: {
  id: string;
  target: ServerTarget;
  revision: number;
  onReport: (id: string, snapshot: PregenRead, target: ServerTarget) => void;
}) {
  const { value, error } = usePoll(() => api.pregen(target.device, target.server), 2000, [
    target.device,
    target.server,
    revision,
  ]);
  useEffect(() => {
    if (!value && !error) return;
    onReport(id, { value, error }, target);
  }, [value, error, id, onReport, target.device, target.server]);
  return null;
}

function DeviceGroup({
  device,
  data,
  selection,
  onSelect,
}: {
  device: Device;
  data: DeviceData | undefined;
  selection: Selection;
  onSelect: (s: Selection) => void;
}) {
  const { value: info } = usePoll(data && !data.error ? () => api.deviceInfo(device.id) : null, 10_000, [
    device.id,
    Boolean(data && !data.error),
  ]);
  const state = !data ? "unknown" : data.error ? "offline" : "online";
  const current = selection.kind === "device" && selection.device === device.id;
  return (
    <div className="device-group">
      <button
        className="device-head"
        aria-current={current}
        onClick={() => onSelect({ kind: "device", device: device.id })}
      >
        <span className="dot" data-state={state} />
        <span className="name">{device.name}</span>
        <span className="route">{routeLabel[device.route]}</span>
      </button>
      {state === "online" && info && <DeviceMeters info={info} />}
      {data?.servers?.map((server, index) => (
        <HostedServerRow
          key={server.name}
          device={device}
          server={server}
          index={index}
          current={
            selection.kind === "server" && selection.device === device.id && selection.server === server.name
          }
          onSelect={() => onSelect({ kind: "server", device: device.id, server: server.name })}
        />
      ))}
    </div>
  );
}

function SidebarFriend({
  friend,
  current,
  onSelect,
}: {
  friend: Friend;
  current: boolean;
  onSelect: () => void;
}) {
  const status = useFriendStatus(friend.address);
  return <FriendRow friend={friend} status={status} current={current} onSelect={onSelect} />;
}

function Mark() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden>
      <g fill="var(--brass)">
        <rect x="3" y="15" width="3" height="6" rx="0.75" />
        <rect x="7.5" y="12" width="3" height="9" rx="0.75" />
        <rect x="12" y="9" width="3" height="12" rx="0.75" />
        <rect x="16.5" y="6" width="3" height="15" rx="0.75" opacity="0.4" />
      </g>
    </svg>
  );
}
