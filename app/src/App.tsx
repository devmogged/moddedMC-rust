import { useCallback, useEffect, useState } from "react";
import { Boxes, Package, Plus } from "lucide-react";
import { api, type Device, type Friend, type ServerInfo } from "./api";
import {
  LoadingRows,
  LoadError,
  ServerCluster,
  ServerIcon,
  packLine,
  routeLabel,
  usePoll,
  useServerIcon,
} from "./ui";
import { DeviceMeters } from "./Metrics";
import { ServerPane } from "./ServerPane";
import { DevicePane } from "./DevicePane";
import { FriendPane, FriendRow, useFriendStatus } from "./FriendPane";
import { ModsPane } from "./ModsPane";
import { PackPane } from "./PackPane";
import { AddDeviceDialog, AddFriendDialog } from "./dialogs";

export type Selection =
  | { kind: "server"; device: string; server: string }
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
    const [kind, a, b] = location.hash.slice(1).split("/");
    if (kind === "server" && a && b) return { kind, device: a, server: b };
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
  const ids = devices.map((d) => d.id).join("|");
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const run = async () => {
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
      timer = window.setTimeout(run, 2000);
    };
    run();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ids, tick]);
  return { data, refresh: useCallback(() => setTick((t) => t + 1), []) };
}

export function App() {
  const devicesPoll = usePoll(api.devices, 60_000, []);
  const friendsPoll = usePoll(api.friends, 60_000, []);
  // Packs are files on this machine; reread them so a build or an edit shows up.
  const packsPoll = usePoll(api.packs, 5_000, []);
  const packs = packsPoll.value ?? [];
  const devices = devicesPoll.value ?? [];
  const friends = friendsPoll.value ?? [];
  const { data, refresh } = useDeviceData(devices);
  const [selection, setSelectionState] = useState<Selection>(loadSelection);
  const [dialog, setDialog] = useState<"device" | "friend" | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const select = (next: Selection) => {
    setSelectionState(next);
    try {
      localStorage.setItem(SELECTION_KEY, JSON.stringify(next));
    } catch {
      // Not remembered across launches; fine.
    }
  };

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 5000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const addPack = async () => {
    try {
      const path = await api.pickPackFile();
      if (!path) return;
      const pack = await api.addPack(path);
      packsPoll.refresh();
      select({ kind: "pack", path: pack.path });
    } catch (err) {
      setToast(err instanceof Error ? err.message : String(err));
    }
  };

  const fail = useCallback((err: unknown) => {
    setToast(err instanceof Error ? err.message : String(err));
  }, []);

  const device = (id: string) => devices.find((d) => d.id === id);

  let pane = null;
  const server =
    selection.kind === "server"
      ? data[selection.device]?.servers?.find((s) => s.name === selection.server)
      : undefined;
  if (selection.kind === "server" && server && device(selection.device)) {
    const d = device(selection.device) as Device;
    pane = (
      <ServerPane
        key={`${d.id}/${server.name}`}
        device={d}
        server={server}
        onChanged={refresh}
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

          <section className="section" aria-label="Mods">
            <button
              className="nav-item"
              aria-current={selection.kind === "mods"}
              onClick={() => select({ kind: "mods" })}
            >
              <Boxes />
              Mods
            </button>
          </section>
        </div>
      </aside>

      <main className="main" aria-label="Workspace">
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

      {toast ? (
        <div className="toast" role="status" data-tone="bad">
          {toast}
        </div>
      ) : null}
    </div>
  );
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
        <SidebarServer
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

function SidebarServer({
  device,
  server,
  index,
  current,
  onSelect,
}: {
  device: Device;
  server: ServerInfo;
  index: number;
  current: boolean;
  onSelect: () => void;
}) {
  const icon = useServerIcon(device, server);
  return (
    <button
      className="row server-identity sidebar-server"
      style={{ ...icon.style, "--enter-delay": Math.min(index, 4) * 25 + "ms" } as React.CSSProperties}
      aria-current={current}
      onClick={onSelect}
    >
      <ServerIcon name={server.name} favicon={icon.favicon} />
      <span className="row-text">
        <span className="row-name">{server.name}</span>
        <span className="row-line">{packLine(server)}</span>
      </span>
      <ServerCluster device={device} server={server} />
    </button>
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
