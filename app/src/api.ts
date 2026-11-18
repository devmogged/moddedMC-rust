import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Route = "local" | "tailscale" | "ssh";

export interface Device {
  id: string;
  name: string;
  route: Route;
  host: string;
  port: number;
}

export interface DeviceInfo {
  name: string;
  os: string;
  arch: string;
  cpus: number;
  ram_total: number;
  ram_available: number;
  disk_free: number;
  agent_version: string;
  tailscale_name: string | null;
}

export type ServerState = "stopped" | "starting" | "running" | "stopping" | "crashed";

export interface ServerInfo {
  name: string;
  state: ServerState;
  pack: string | null;
  port: number | null;
  pid: number | null;
  uptime_secs: number | null;
  exit_code: number | null;
}

export interface Stats {
  state: ServerState;
  rss_bytes: number | null;
  cpu_percent: number | null;
  uptime_secs: number | null;
}

export interface Friend {
  id: string;
  name: string;
  address: string;
}

export interface Status {
  latency_ms: number;
  motd: string;
  version: string | null;
  players_online: number | null;
  players_max: number | null;
  favicon: string | null;
}

export interface ModEntry {
  id: string;
  name: string;
  version: string;
  kind: string;
  minecraft: string | null;
  upstream: string | null;
}

// Outside Tauri (a plain browser on `npm run dev`) a sample backend stands in.
const mock = import.meta.env.DEV && !("__TAURI_INTERNALS__" in window) ? await import("./mock") : null;

/** Tauri rejects with the backend's error string; keep it as an Error for callers. */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    if (mock) return (await mock.mockInvoke(command, args)) as T;
    return await invoke<T>(command, args);
  } catch (err) {
    throw new Error(typeof err === "string" ? err : String(err));
  }
}

const agent = <T>(device: string, method: "GET" | "POST", path: string, body?: unknown) =>
  call<T>("agent", { device, method, path, body });

export const api = {
  devices: () => call<Device[]>("devices_list"),
  addDevice: (device: { name: string; route: Route; host: string; port?: number }) =>
    call<Device>("device_add", { device }),
  removeDevice: (id: string) => call<void>("device_remove", { id }),

  deviceInfo: (device: string) => agent<DeviceInfo>(device, "GET", "/v1/device"),
  servers: (device: string) => agent<ServerInfo[]>(device, "GET", "/v1/servers"),
  createServer: (device: string, server: { name: string; exe: string; port?: number }) =>
    agent<void>(device, "POST", "/v1/servers", server),
  action: (device: string, server: string, action: "start" | "stop" | "restart") =>
    agent<void>(device, "POST", `/v1/servers/${encodeURIComponent(server)}/${action}`),
  stats: (device: string, server: string) =>
    agent<Stats>(device, "GET", `/v1/servers/${encodeURIComponent(server)}/stats`),

  consoleOpen: (device: string, server: string) => call<number>("console_open", { device, server }),
  consoleSend: (id: number, line: string) => call<void>("console_send", { id, line }),
  consoleClose: (id: number) => call<void>("console_close", { id }),
  onConsole: (handler: (event: { id: number; line: string | null }) => void): Promise<UnlistenFn> =>
    mock
      ? Promise.resolve(mock.mockListen(handler))
      : listen<{ id: number; line: string | null }>("console", (e) => handler(e.payload)),

  friends: () => call<Friend[]>("friends_list"),
  addFriend: (friend: { name: string; address: string }) => call<Friend>("friend_add", { friend }),
  removeFriend: (id: string) => call<void>("friend_remove", { id }),
  ping: (address: string) => call<Status>("ping", { address }),

  mods: () => call<ModEntry[]>("mods_index"),
};

/** The address players use for a server on a device. */
export function minecraftAddress(device: Device, server: ServerInfo): string {
  const port = server.port ?? 25565;
  const host =
    device.route === "local"
      ? "127.0.0.1"
      : device.route === "tailscale"
        ? device.host
        : (device.host.split("@").pop() ?? device.host);
  return port === 25565 ? host : `${host}:${port}`;
}
