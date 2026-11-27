import type { ServerConfig, ServerInfo } from "./api";

// Browser preview backend: lets `npm run dev` render the app outside Tauri with sample
// devices, servers and a console. Only loaded in dev builds when Tauri is absent.

type Handler = (event: { id: number; line: string | null }) => void;

const started = Date.now();
const scenario = new URLSearchParams(location.search).get("mock");
const listeners = new Set<Handler>();
let consoles = 0;
const state: Partial<Record<string, ServerInfo["state"]>> = {
  survival: "running",
  create: "stopped",
  "boss-rush": "crashed",
};

if (scenario === "starting") state.survival = "starting";
const log = [
  "[00:00:01] INFO Pumpkin 0.1.0 for Minecraft 26.3",
  "[00:00:01] INFO Loading 64 drop-in mods",
  '[00:00:02] INFO Preparing level "world"',
  "[00:00:04] WARN Chunk 12,-4 took 61 ms to generate",
  "[00:00:05] INFO Started server in 4.2 s",
  "[00:03:12] INFO sting joined the game",
  "[00:05:40] INFO noel joined the game",
];

function health(base: number) {
  const mspt = base + Math.sin(Date.now() / 5000) * base * 0.15;
  return {
    mspt,
    mspt_max: mspt * 1.8,
    tps: Math.min(20, 1000 / mspt),
    tickrate: 20,
    players: 3,
    max_players: 20,
  };
}

function sampleServers(device: string): ServerInfo[] {
  if (device === "pi") {
    if (scenario === "empty") return [];
    return [
      {
        name: "survival",
        state: state.survival ?? "stopped",
        pack: "buja",
        port: 25565,
        pid: 812,
        uptime_secs: (Date.now() - started) / 1000 + 86000,
        exit_code: null,
        health: state.survival === "running" ? health(scenario === "degraded" ? 44 : 14) : null,
        external: false,
        program: null,
        folder: null,
      },
      {
        name: "boss-rush",
        state: state["boss-rush"] ?? "stopped",
        pack: "bosses",
        port: 25566,
        pid: null,
        uptime_secs: null,
        exit_code: 101,
        health: state["boss-rush"] === "running" ? health(41) : null,
        external: false,
        program: null,
        folder: null,
      },
    ];
  }
  return [
    {
      name: "create",
      state: state.create ?? "stopped",
      pack: "create-test",
      port: 25570,
      pid: null,
      uptime_secs: null,
      exit_code: 0,
      health: state.create === "running" ? health(6) : null,
      external: false,
      program: null,
      folder: null,
    },
    {
      name: "old-paper",
      state: "running",
      pack: "Paper 1.21.4",
      port: 25580,
      pid: 15724,
      uptime_secs: null,
      exit_code: null,
      health: null,
      external: true,
      program: "java",
      folder: "D:/servers/old-paper",
    },
    {
      name: "fabric-test",
      state: "stopped",
      pack: "fabric-server-launch",
      port: 25590,
      pid: null,
      uptime_secs: null,
      exit_code: null,
      health: null,
      external: true,
      program: null,
      folder: "D:/servers/fabric-test",
    },
  ];
}

const serverLists = new Map<string, ServerInfo[]>();
const config = new Map<string, ServerConfig>();
function servers(device: string) {
  let list = serverLists.get(device);
  if (!list) {
    list = sampleServers(device);
    serverLists.set(device, list);
  }
  return list;
}

const DEFAULT_CONFIG: ServerConfig = {
  java: true,
  bedrock: false,
  online_mode: false,
  max_players: 1000,
  motd: "A blazingly fast Pumpkin server!",
  view_distance: 16,
  simulation_distance: 10,
  difficulty: "Normal",
  gamemode: "Survival",
  hardcore: false,
  white_list: false,
  allow_nether: true,
  allow_end: true,
  spawn_protection: 16,
  icon: null,
};

let pregenState = scenario === "pregen-idle" || scenario === "pregen-fast" ? "idle" : "running";
let pregenStarted = started;
let pregenPercent = 42;
const backupLists = new Map<
  string,
  { id: string; created: number; files: number; bytes: number; stored: number; scheduled: boolean }[]
>();
let mockSchedule: { every_minutes: number; keep: number } | null = { every_minutes: 360, keep: 8 };
let mockPackMods = [
  { id: "counter", source: "path" },
  { id: "port-example", source: "mmc" },
  { id: "wayshrine", source: "git" },
  { id: "chunky", source: "index" },
];
const serverMods: Record<string, { file: string; id: string; version: string; bytes: number }[]> = {};

export async function mockInvoke(command: string, args: Record<string, any> = {}): Promise<unknown> {
  await new Promise((r) => setTimeout(r, scenario === "loading" && command === "devices_list" ? 5000 : 60));
  switch (command) {
    case "runtime_server_exe":
      return "pack-dev.exe";
    case "devices_list":
      return [
        { id: "local", name: "This PC", route: "local", host: "", port: 25599 },
        { id: "pi", name: "Pi", route: "tailscale", host: "pi", port: 25599 },
        { id: "nas", name: "Basement box", route: "ssh", host: "noel@192.168.1.20", port: 25599 },
      ];
    case "agent": {
      const { device, method, path, body } = args as {
        device: string;
        method: string;
        path: string;
        body?: unknown;
      };
      if (device === "nas") throw "Can't reach Basement box. Is the agent running there?";
      if (path === "/v1/device")
        return {
          name: device === "pi" ? "pi" : "desktop",
          os: device === "pi" ? "Debian GNU/Linux 12" : "Windows 11 Home",
          arch: device === "pi" ? "aarch64" : "x86_64",
          cpus: device === "pi" ? 4 : 16,
          ram_total: (device === "pi" ? 8 : 32) * 2 ** 30,
          ram_available: (device === "pi" ? 6.9 : 18.2) * 2 ** 30,
          cpu_percent: (device === "pi" ? 23 : 11) + Math.sin(Date.now() / 5000) * 6,
          disk_total: (device === "pi" ? 512 : 1024) * 2 ** 30,
          disk_free: (device === "pi" ? 412 : 630) * 2 ** 30,
          agent_version: "0.1.0",
          tailscale_name: device === "pi" ? "pi.tail1234.ts.net" : null,
        };
      if (path === "/v1/servers") {
        if (method === "GET") return servers(device).map((server) => ({ ...server }));
        const { name, port } = body as { name: string; port: number };
        if (servers(device).some((server) => server.name === name))
          throw "A server with this name already exists.";
        servers(device).push({
          name,
          port,
          state: "stopped",
          pack: null,
          pid: null,
          uptime_secs: null,
          exit_code: null,
          health: null,
          external: false,
          program: null,
          folder: null,
        });
        return null;
      }
      const settings = path.match(/^\/v1\/servers\/([^/]+)(?:\/(port|config))?$/);
      if (settings) {
        const name = decodeURIComponent(settings[1]);
        const key = device + "/" + name;
        const server = servers(device).find((server) => server.name === name && !server.external);
        if (!server) throw "Server not found.";
        if (settings[2] === "config") {
          if (method === "GET") return config.get(key) ?? DEFAULT_CONFIG;
          config.set(key, body as ServerConfig);
        } else {
          if (server.state !== "stopped" && server.state !== "crashed") throw "Stop the server first.";
          if (method === "DELETE") {
            config.delete(key);
            serverLists.set(
              device,
              servers(device).filter((item) => item !== server),
            );
          } else {
            server.port = (body as { port: number }).port;
            server.start_error = null;
          }
        }
        return null;
      }
      if (path === "/v1/builds")
        return device === "pi" ? [{ id: "example-3f9a1c2b7d4e", pack: "example", version: "1" }] : [];
      const mods = path.match(/^\/v1\/servers\/([^/]+)\/mods(?:\/(.+))?$/);
      if (mods) {
        const list = (serverMods[mods[1]] ??= [
          { file: "counter.mmc", id: "counter", version: "0.2.0", bytes: 59535 },
          { file: "wayshrine.mmc", id: "wayshrine", version: "0.2.0", bytes: 99783 },
        ]);
        if (method === "DELETE")
          serverMods[mods[1]] = list.filter((m) => m.file !== decodeURIComponent(mods[2]));
        return method === "GET" ? list : null;
      }
      if (path.endsWith("/backups")) {
        const key = device + path;
        let backups = backupLists.get(key);
        if (!backups)
          backups = [
            {
              id: "1795030000",
              created: 1795030000,
              files: 412,
              bytes: 182 * 2 ** 20,
              stored: 3.1 * 2 ** 20,
              scheduled: true,
            },
            {
              id: "1794940000",
              created: 1794940000,
              files: 405,
              bytes: 176 * 2 ** 20,
              stored: 151 * 2 ** 20,
              scheduled: false,
            },
          ];
        backupLists.set(key, backups);
        if (method === "GET") return backups;
        await new Promise((r) => setTimeout(r, 350));
        const created = Math.floor(Date.now() / 1000);
        const backup = {
          id: String(Date.now()),
          created,
          files: 419,
          bytes: 183 * 2 ** 20,
          stored: 3.3 * 2 ** 20,
          scheduled: false,
        };
        backups.unshift(backup);
        return backup;
      }
      if (path.endsWith("/backups/schedule")) {
        if (method === "PUT") mockSchedule = (body as typeof mockSchedule) ?? null;
        return mockSchedule;
      }
      if (path.includes("/backups")) return null;
      if (path.endsWith("/pregen") && method === "GET") {
        const pregenDuration =
          scenario === "pregen-fast" ? 40 : scenario === "pregen-finish" ? 4500 : 120_000;
        let percent = pregenPercent;
        if (pregenState === "running") {
          percent = Math.min(100, pregenPercent + (100 * (Date.now() - pregenStarted)) / pregenDuration);
          if (percent >= 100) pregenState = "finished";
        }
        if (pregenState === "idle")
          return { state: "idle", world: null, chunks: null, percent: null, eta_secs: null, rate: null };
        return {
          state: pregenState,
          world: "minecraft:overworld",
          chunks: Math.round((16129 * percent) / 100),
          percent,
          eta_secs: Math.round((pregenDuration / 1000) * (1 - percent / 100)),
          rate: 543,
        };
      }
      if (path.includes("/pregen")) {
        if (path.endsWith("/pause")) {
          pregenPercent = Math.min(100, pregenPercent + (100 * (Date.now() - pregenStarted)) / 120_000);
          pregenState = "paused";
        } else if (path.endsWith("/cancel")) pregenState = "cancelled";
        else {
          if (!path.endsWith("/continue")) pregenPercent = 0;
          pregenStarted = Date.now();
          pregenState = "running";
        }
        return null;
      }
      const match = path.match(/^\/v1\/servers\/([^/]+)\/(\w+)$/);
      if (match && method === "POST") {
        const name = decodeURIComponent(match[1]);
        const action = match[2];
        const list = servers(device);
        const server = list.find((server) => server.name === name);
        if (!server) throw "Server not found.";
        if (action === "start" || action === "restart") {
          const owner = list.find(
            (other) =>
              other !== server &&
              other.port === server.port &&
              (other.state === "running" || other.state === "starting"),
          );
          if (owner) {
            server.start_error =
              "Server cannot start. Java port " +
              server.port +
              " is in use by " +
              owner.name +
              ". Choose another port in Settings or free this port.";
            server.state = "stopped";
            throw server.start_error;
          }
        }
        if (scenario === "action-error") throw "Could not start " + name + ".";
        server.start_error = null;
        server.state = action === "stop" ? "stopping" : "starting";
        setTimeout(() => {
          server.state = action === "stop" ? "stopped" : "running";
          state[name] = server.state;
        }, 2500);
        return null;
      }
      if (match?.[2] === "stats")
        return {
          state: state[match[1]],
          rss_bytes: 612 * 2 ** 20 + Math.sin(Date.now() / 9000) * 40 * 2 ** 20,
          cpu_percent: 18 + Math.sin(Date.now() / 4000) * 9,
          uptime_secs: (Date.now() - started) / 1000 + 86000,
          health:
            state[match[1]] === "running"
              ? health(match[1] === "survival" ? (scenario === "degraded" ? 44 : 14) : 41)
              : null,
        };
      return null;
    }
    case "console_open": {
      const id = ++consoles;
      const serverLog =
        args.server === "boss-rush"
          ? [log[0], "[00:00:03] ERROR Server exited with code 101"]
          : args.server === "create" && state.create === "stopped"
            ? []
            : log;
      setTimeout(() => serverLog.forEach((line) => listeners.forEach((l) => l({ id, line }))), 30);
      return id;
    }
    case "console_send": {
      const { id, line } = args as { id: number; line: string };
      setTimeout(() => listeners.forEach((l) => l({ id, line: `[00:06:01] INFO ran ${line}` })), 120);
      return null;
    }
    case "console_close":
      return null;
    case "packs_list":
    case "pack_add":
    case "pack_add_mod":
    case "pack_remove_mod": {
      if (command === "pack_add_mod") mockPackMods.push({ id: "create", source: "path" });
      if (command === "pack_remove_mod") mockPackMods = mockPackMods.filter((m) => m.id !== args.id);
      const pack = {
        path: "C:/moddedMC-rust/packs/example/pack.toml",
        id: "example",
        name: "Example",
        version: "1",
        mods: mockPackMods,
        targets: [
          {
            name: "laptop",
            triple: "x86_64-pc-windows-msvc",
            built: "C:/moddedMC-rust/target/packs/example/laptop",
          },
          { name: "pi", triple: "aarch64-unknown-linux-gnu", built: null },
        ],
        error: null,
      };
      return command === "packs_list" ? [pack] : pack;
    }
    case "pack_build":
      return "C:/moddedMC-rust/target/packs/example/" + args.target;
    case "pack_remove":
      return null;
    case "install_agent":
      return "Installed and running.";
    case "mod_upload":
      return null;
    case "build_inspect":
    case "deploy":
      return { id: "example-3f9a1c2b7d4e", pack: "example", version: "1", path: args.path };
    case "mods_index":
      return scenario === "empty"
        ? []
        : [
            {
              id: "chunky",
              name: "Chunky",
              version: "1.0.0",
              kind: "drop-in",
              minecraft: "26.3",
              upstream: null,
            },
            {
              id: "wayshrine",
              name: "Wayshrine",
              version: "0.2.0",
              kind: "drop-in",
              minecraft: "26.3",
              upstream: null,
            },
            {
              id: "counter",
              name: "Counter",
              version: "0.2.0",
              kind: "drop-in",
              minecraft: "26.3",
              upstream: null,
            },
          ];
    default:
      return null;
  }
}

export function mockListen(handler: Handler): () => void {
  listeners.add(handler);
  return () => listeners.delete(handler);
}
