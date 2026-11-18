# Devices: the agent and how the app reaches it

Every machine that hosts servers (the laptop, the Pi, a LAN box) runs **`moddedmc-agent`**, a small daemon. The desktop app talks only to agents, including the one on its own machine, so local and remote servers work the same way. This follows T3 Code's model: each machine runs its own server, and the app keeps an ordered list of routes to it and uses the first one that works.

## Trust (no tokens)

Only the owner controls servers. The agent decides who the caller is from how they arrived:

| Route | How the app connects | Who is trusted |
| --- | --- | --- |
| Local | `http://127.0.0.1:25599` | Anyone on loopback (it's this machine). |
| SSH | The app opens `ssh -N -L <port>:127.0.0.1:25599 <host>` with the user's own SSH config and keys, then uses the loopback route. | Loopback, as above. Whoever can SSH in already owns the box. |
| Tailscale | `http://<tailnet name or 100.x address>:25599` | The agent asks the local `tailscale` daemon who the peer is (`tailscale whois --json <ip>`). It accepts only the owner's Tailscale login (`owner` in `agent.toml`, set to this device's own login on first run). Friends sharing the node can play but cannot control. |

The agent listens on loopback and on its tailnet address only, never on `0.0.0.0`, so a plain LAN can't reach it. LAN-only machines (no Tailscale) are reached over SSH.

## Layout on a device

```
~/.moddedmc/
  agent.toml              owner login, listen port, servers root
  servers/<name>/         one folder per server: the pack server binary, mods/, config/, world/
    server.json           pack id and version, target, created, last started
    stats.json            written by the pack server every 2 s (MSPT, TPS, players) when started with MODDEDMC_STATS=1
    pumpkin.toml          the server's config; the agent sets only the Java port (from server.json) and, on create, offline mode, telemetry off and no Bedrock listener
  builds/<pack>-<hash>/   pack builds pushed by the app (binary + pack.lock), shared by servers
  backups/<name>/         content-addressed snapshots (see W3)
```

## API (HTTP + JSON, one WebSocket per console)

| Method | Path | What |
| --- | --- | --- |
| GET | `/v1/device` | Name, OS, arch, CPU count, RAM, disk free, agent version, Tailscale name. |
| GET | `/v1/servers` | Every server folder: name, pack, state (`stopped`, `starting`, `running`, `stopping`, `crashed`), PID, port, players online. Also lists Minecraft servers running on the machine that the agent didn't start (a Java or Pumpkin process listening on a port), marked `external`. |
| POST | `/v1/servers` | Create a server from a build: `{name, build, port}`. |
| POST | `/v1/servers/<name>/start`, `/stop`, `/restart` | Lifecycle. `stop` sends `stop` on the console and waits; kill only after a timeout. |
| GET | `/v1/servers/<name>/stats?window=60` | Time series: MSPT p50/p95/p99, TPS, RSS, CPU, players, chunks loaded, network bytes in and out. |
| WS | `/v1/servers/<name>/console` | Log lines out (with the last 500 on connect), console commands in. |
| PUT | `/v1/builds/<pack>-<hash>` | Upload a pack build (tar.zst). Idempotent: an existing hash returns 200 at once. |
| PUT | `/v1/servers/<name>/mods/<file>`, DELETE likewise | Drop-in mods. |
| POST | `/v1/servers/<name>/backups`, GET likewise | Take and list snapshots. |
| POST | `/v1/servers/<name>/pregen` | Start pregeneration with `{radius, center}`; progress comes over the console socket. |

Stats come from the server itself: pack servers print one machine-readable stats line per second when started with `MODDEDMC_STATS=1` (the host already has `stats::attach`). The agent never polls with game commands.

## Install

The app installs the agent:

- Local: it runs the bundled binary as a user service.
- Over SSH: it copies the binary for that machine's target (`aarch64-unknown-linux-gnu` on the Pi), writes `agent.toml` with the owner set to the app user's Tailscale login, and enables a systemd user service (`loginctl enable-linger` so it runs without a login session).

Nothing else is installed on the device. Pack builds are made on the laptop and pushed.
