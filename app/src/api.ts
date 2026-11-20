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
  cpu_percent?: number | null;
  disk_total?: number | null;
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
  health: Health | null;
  /** Running on the device, but not started by its agent: shown, not controlled. */
  external: boolean;
  program: string | null;
  /** Where an external server lives. */
  folder: string | null;
}

export interface Health {
  mspt: number;
  mspt_max: number;
  tps: number;
  tickrate: number;
  players: number;
  max_players: number;
}

export interface Stats {
  state: ServerState;
  rss_bytes: number | null;
  cpu_percent: number | null;
  uptime_secs: number | null;
  health: Health | null;
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

export interface Build {
  id: string;
  pack: string;
  version: string;
  path: string;
}

export interface Pack {
  path: string;
  id: string;
  name: string;
  version: string;
  mods: { id: string; source: string }[];
  targets: { name: string; triple: string; built: string | null }[];
  error: string | null;
}

export interface UploadedBuild {
  id: string;
  pack: string;
  version: string;
}

export interface ModEntry {
  id: string;
  name: string;
  version: string;
  kind: string;
  description: string | null;
  minecraft: string | null;
  upstream: string | null;
  license: string | null;
  /** The built .mmc; absent for mods that only come compiled into packs. */
  download: string | null;
  sha256: string | null;
  size: number | null;
  /** The ported release, like "Saint's Dragons 0.9.88". */
  original: string | null;
  /** Mod ids installed along with this one. */
  dependencies: string[];
  icon: string | null;
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

const agent = <T>(device: string, method: "GET" | "POST" | "PUT" | "DELETE", path: string, body?: unknown) =>
  call<T>("agent", { device, method, path, body });

/** A pregeneration task on a server with the Chunky port. */
export type Pregen = {
  state: "idle" | "running" | "paused" | "finished" | "cancelled";
  world: string | null;
  chunks: number | null;
  percent: number | null;
  eta_secs: number | null;
  rate: number | null;
};

/** A snapshot of a server folder in the device's backup store. */
export type Backup = {
  id: string;
  created: number;
  files: number;
  bytes: number;
  stored: number;
  scheduled: boolean;
};
export type BackupSchedule = { every_minutes: number; keep: number };

/** Where friends can join a server (agent `/addresses`). */
export interface Addresses {
  addresses: { kind: "playit" | "tailscale" | "lan"; address: string }[];
  playit: "signed-out" | "no-tunnel" | "stopped" | "running" | "unknown";
}

/** The `pumpkin.toml` settings the app edits (agent `/config`); they apply on the next start. */
export type ServerConfig = {
  java: boolean;
  bedrock: boolean;
  online_mode: boolean;
  max_players: number;
  motd: string;
  view_distance: number;
  simulation_distance: number;
  difficulty: "Peaceful" | "Easy" | "Normal" | "Hard";
  gamemode: "Survival" | "Creative" | "Adventure" | "Spectator";
  hardcore: boolean;
  white_list: boolean;
  allow_nether: boolean;
  allow_end: boolean;
  spawn_protection: number;
  /** PNG bytes of the server-list icon. */
  icon: number[] | null;
};

/** A drop-in mod in a server's `mods/`. */
export type ServerMod = { file: string; id: string | null; version: string | null; bytes: number };

export type RuntimeStatus = { installed: string | null; latest: string | null; error: string | null };

export const api = {
  devices: () => call<Device[]>("devices_list"),
  addDevice: (device: { name: string; route: Route; host: string; port?: number }) =>
    call<Device>("device_add", { device }),
  removeDevice: (id: string) => call<void>("device_remove", { id }),
  /** Installs the agent on a Linux machine over SSH; resolves to a one-line result. */
  installAgent: (destination: string) => call<string>("install_agent", { destination }),

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
  joinFriend: (id: string) => call<string>("friend_join", { id }),
  ping: (address: string) => call<Status>("ping", { address }),

  mods: () => call<ModEntry[]>("mods_index"),
  installMod: (device: string, server: string, id: string) =>
    call<void>("mod_install", { device, server, id }),

  runtimeStatus: () => call<RuntimeStatus>("runtime_status"),
  /** Downloads the server and agent; progress arrives through `onRuntimeProgress`. */
  installRuntime: () => call<string>("runtime_install"),
  runtimeServerExe: () => call<string>("runtime_server_exe"),
  ensureAgent: () => call<void>("agent_ensure"),
  onRuntimeProgress: (
    handler: (event: { role: string; received: number; total: number }) => void,
  ): Promise<UnlistenFn> =>
    mock
      ? Promise.resolve(() => {})
      : listen<{ role: string; received: number; total: number }>("runtime-progress", (e) =>
          handler(e.payload),
        ),

  packs: () => call<Pack[]>("packs_list"),
  addPack: (path: string) => call<Pack>("pack_add", { path }),
  removePack: (path: string) => call<void>("pack_remove", { path }),
  addPackMod: (path: string, source: string) => call<Pack>("pack_add_mod", { path, source }),
  removePackMod: (path: string, id: string) => call<Pack>("pack_remove_mod", { path, id }),
  /** Builds one target; resolves to the build folder. Output arrives through `onPackBuild`. */
  buildPack: (path: string, target: string) => call<string>("pack_build", { path, target }),
  onPackBuild: (
    handler: (event: { path: string; target: string; line: string }) => void,
  ): Promise<UnlistenFn> =>
    mock
      ? Promise.resolve(() => {})
      : listen<{ path: string; target: string; line: string }>("pack-build", (e) => handler(e.payload)),
  uploadedBuilds: (device: string) => agent<UploadedBuild[]>(device, "GET", "/v1/builds"),
  /** A `pack.toml` picker; `null` when the user cancels. */
  pickPackFile: async (): Promise<string | null> => {
    if (mock) return "C:/moddedMC-rust/packs/example/pack.toml";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "Pack", extensions: ["toml"] }] });
    return typeof picked === "string" ? picked : null;
  },

  serverMods: (device: string, server: string) =>
    agent<ServerMod[]>(device, "GET", `/v1/servers/${encodeURIComponent(server)}/mods`),
  addMod: (device: string, server: string, path: string) =>
    call<void>("mod_upload", { device, server, path }),
  removeMod: (device: string, server: string, file: string) =>
    agent<void>(
      device,
      "DELETE",
      `/v1/servers/${encodeURIComponent(server)}/mods/${encodeURIComponent(file)}`,
    ),
  addresses: (device: string, server: string) =>
    agent<Addresses>(device, "GET", `/v1/servers/${encodeURIComponent(server)}/addresses`),
  playitStatus: (device: string) =>
    agent<{ signed_in: boolean; running: boolean; claim: { url: string; error: string | null } | null }>(
      device,
      "GET",
      "/v1/playit",
    ),
  playitClaim: (device: string) => agent<{ url: string | null }>(device, "POST", "/v1/playit/claim"),
  playitSignOut: (device: string) => agent<void>(device, "POST", "/v1/playit/sign-out"),
  startPlayit: (device: string, server: string) =>
    agent<void>(device, "POST", `/v1/servers/${encodeURIComponent(server)}/playit`),
  config: (device: string, server: string) =>
    agent<ServerConfig>(device, "GET", `/v1/servers/${encodeURIComponent(server)}/config`),
  setConfig: (device: string, server: string, config: ServerConfig) =>
    agent<void>(device, "PUT", `/v1/servers/${encodeURIComponent(server)}/config`, config),
  backups: (device: string, server: string) =>
    agent<Backup[]>(device, "GET", `/v1/servers/${encodeURIComponent(server)}/backups`),
  backUp: (device: string, server: string) =>
    agent<Backup>(device, "POST", `/v1/servers/${encodeURIComponent(server)}/backups`),
  backupSchedule: (device: string, server: string) =>
    agent<BackupSchedule | null>(device, "GET", `/v1/servers/${encodeURIComponent(server)}/backups/schedule`),
  setBackupSchedule: (device: string, server: string, schedule: BackupSchedule | null) =>
    agent<BackupSchedule | null>(
      device,
      "PUT",
      `/v1/servers/${encodeURIComponent(server)}/backups/schedule`,
      schedule,
    ),
  restoreBackup: (device: string, server: string, id: string) =>
    agent<void>(device, "POST", `/v1/servers/${encodeURIComponent(server)}/backups/${id}/restore`),
  removeBackup: (device: string, server: string, id: string) =>
    agent<void>(device, "DELETE", `/v1/servers/${encodeURIComponent(server)}/backups/${id}`),
  pregen: (device: string, server: string) =>
    agent<Pregen>(device, "GET", `/v1/servers/${encodeURIComponent(server)}/pregen`),
  startPregen: (
    device: string,
    server: string,
    task: { radius: number; center_x: number; center_z: number; circle: boolean },
  ) => agent<void>(device, "POST", `/v1/servers/${encodeURIComponent(server)}/pregen`, task),
  pregenAction: (device: string, server: string, action: "pause" | "continue" | "cancel") =>
    agent<void>(device, "POST", `/v1/servers/${encodeURIComponent(server)}/pregen/${action}`),
  pickModFile: async (): Promise<string | null> => {
    if (mock) return "C:/mods/counter.mmc";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "Mod", extensions: ["mmc"] }] });
    return typeof picked === "string" ? picked : null;
  },

  inspectBuild: (path: string) => call<Build>("build_inspect", { path }),
  deploy: (device: string, server: string, path: string) => call<Build>("deploy", { device, server, path }),
  /** A folder picker; `null` when the user cancels. */
  pickFolder: async (): Promise<string | null> => {
    if (mock) return "C:/moddedMC-rust/target/packs/example/laptop";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
  },
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
