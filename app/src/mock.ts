// Browser preview backend: lets `npm run dev` render the app outside Tauri with sample
// devices, servers and a console. Only loaded in dev builds when Tauri is absent.

type Handler = (event: { id: number; line: string | null }) => void;

const started = Date.now();
const listeners = new Set<Handler>();
let consoles = 0;
const state: Record<string, string> = { survival: "running", create: "stopped", "boss-rush": "crashed" };

const log = [
  "[00:00:01] INFO Pumpkin 0.1.0 for Minecraft 26.3",
  "[00:00:01] INFO Loading 64 mods (12 drop-in, 52 compiled in)",
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

function servers(device: string) {
  if (device === "pi") {
    return [
      {
        name: "survival",
        state: state.survival,
        pack: "buja",
        port: 25565,
        pid: 812,
        uptime_secs: (Date.now() - started) / 1000 + 86000,
        exit_code: null,
        health: health(14),
        external: false,
        program: null,
      },
      {
        name: "boss-rush",
        state: state["boss-rush"],
        pack: "bosses",
        port: 25566,
        pid: null,
        uptime_secs: null,
        exit_code: 101,
        health: state["boss-rush"] === "running" ? health(41) : null,
        external: false,
        program: null,
      },
    ];
  }
  return [
    {
      name: "create",
      state: state.create,
      pack: "create-test",
      port: 25570,
      pid: null,
      uptime_secs: null,
      exit_code: 0,
      health: state.create === "running" ? health(6) : null,
      external: false,
      program: null,
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
    },
  ];
}

export async function mockInvoke(command: string, args: Record<string, any> = {}): Promise<unknown> {
  await new Promise((r) => setTimeout(r, 60));
  switch (command) {
    case "devices_list":
      return [
        { id: "local", name: "This PC", route: "local", host: "", port: 25599 },
        { id: "pi", name: "Pi", route: "tailscale", host: "pi", port: 25599 },
        { id: "nas", name: "Basement box", route: "ssh", host: "noel@192.168.1.20", port: 25599 },
      ];
    case "agent": {
      const { device, method, path } = args as { device: string; method: string; path: string };
      if (device === "nas") throw "Can't reach Basement box. Is the agent running there?";
      if (path === "/v1/device")
        return {
          name: device === "pi" ? "pi" : "desktop",
          os: device === "pi" ? "Debian GNU/Linux 12" : "Windows 11 Home",
          arch: device === "pi" ? "aarch64" : "x86_64",
          cpus: device === "pi" ? 4 : 16,
          ram_total: (device === "pi" ? 8 : 32) * 2 ** 30,
          ram_available: (device === "pi" ? 6.9 : 18.2) * 2 ** 30,
          disk_free: 412 * 2 ** 30,
          agent_version: "0.1.0",
          tailscale_name: device === "pi" ? "pi.tail1234.ts.net" : null,
        };
      if (path === "/v1/servers") return servers(device);
      if (path === "/v1/builds")
        return device === "pi" ? [{ id: "example-3f9a1c2b7d4e", pack: "example", version: "1" }] : [];
      const match = path.match(/^\/v1\/servers\/([^/]+)\/(\w+)$/);
      if (match && method === "POST") {
        const [, name, action] = match;
        state[name] = action === "stop" ? "stopping" : "starting";
        setTimeout(() => (state[name] = action === "stop" ? "stopped" : "running"), 2500);
        return null;
      }
      if (match?.[2] === "stats")
        return {
          state: state[match[1]],
          rss_bytes: 612 * 2 ** 20 + Math.sin(Date.now() / 9000) * 40 * 2 ** 20,
          cpu_percent: 18 + Math.sin(Date.now() / 4000) * 9,
          uptime_secs: (Date.now() - started) / 1000 + 86000,
          health: state[match[1]] === "running" ? health(match[1] === "survival" ? 14 : 41) : null,
        };
      return null;
    }
    case "console_open": {
      const id = ++consoles;
      setTimeout(() => log.forEach((line) => listeners.forEach((l) => l({ id, line }))), 30);
      return id;
    }
    case "console_send": {
      const { id, line } = args as { id: number; line: string };
      setTimeout(() => listeners.forEach((l) => l({ id, line: `[00:06:01] INFO ran ${line}` })), 120);
      return null;
    }
    case "console_close":
      return null;
    case "friends_list":
      return [
        { id: "sting", name: "sting's world", address: "sting.example.net" },
        { id: "old", name: "Old SMP", address: "smp.example.org:25570" },
      ];
    case "ping": {
      const address = String(args.address);
      if (address.startsWith("smp.")) throw `${address} is offline.`;
      if (address.startsWith("127.") || address === "pi")
        return {
          latency_ms: 4,
          motd: "",
          version: "26.3",
          players_online: 3,
          players_max: 20,
          favicon: null,
        };
      return {
        latency_ms: 87,
        motd: "Create, dragons and too many bosses",
        version: "26.3",
        players_online: 2,
        players_max: 10,
        favicon: null,
      };
    }
    case "packs_list":
    case "pack_add": {
      const pack = {
        path: "C:/moddedMC-rust/packs/example/pack.toml",
        id: "example",
        name: "Example",
        version: "1",
        mods: [
          { id: "counter", source: "path" },
          { id: "port-example", source: "path" },
          { id: "wayshrine", source: "path" },
        ],
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
      return command === "pack_add" ? pack : [pack];
    }
    case "pack_build":
      return "C:/moddedMC-rust/target/packs/example/" + args.target;
    case "pack_remove":
      return null;
    case "install_agent":
      return "Installed and running.";
    case "build_inspect":
    case "deploy":
      return { id: "example-3f9a1c2b7d4e", pack: "example", version: "1", path: args.path };
    case "mods_index":
      return [];
    default:
      return null;
  }
}

export function mockListen(handler: Handler): () => void {
  listeners.add(handler);
  return () => listeners.delete(handler);
}
