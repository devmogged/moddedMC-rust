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
  agent.toml              owner login, listen port, servers root, find_in (folders holding servers the agent didn't make)
  external.json           folders external servers ran in, so they still show once stopped
  servers/<name>/         one folder per server: the pack server binary, mods/, config/, world/
    server.json           pack id and version, target, created, last started
    stats.json            written by the pack server every 2 s (MSPT, TPS, players) when started with MODDEDMC_STATS=1
    pumpkin.toml          the server's config; the agent sets only the Java port (from server.json) and, on create, offline mode, telemetry off and no Bedrock listener
  builds/<pack>-<hash>/   pack builds pushed by the app (binary + pack.lock), shared by servers
  backups/blobs/          chunks of backed-up files, zstd, named by SHA-256, shared by every server
  backups/snapshots/<name>/<id>.json  one manifest per backup: each file's chunk list
  backups/schedules.json  automatic backups per server: interval and how many to keep
```

## API (HTTP + JSON, one WebSocket per console)

| Method | Path | What |
| --- | --- | --- |
| GET | `/v1/device` | Name, OS, arch, CPU count and load (`cpu_percent`, since the previous request), RAM, disk free and total (the disk holding the servers), agent version, Tailscale name. |
| GET | `/v1/servers` | Every server folder: name, pack, state (`stopped`, `starting`, `running`, `stopping`, `crashed`), PID, port, players online. Also lists Minecraft servers running on the machine that the agent didn't start: a Java (`*.jar` or `nogui`) or Pumpkin process with a listening TCP port that answers the status ping, marked `external: true` with `program` and `folder`. Scanned at most every 10 s. Stopped external servers are listed too: folders an external server was seen running in (kept in `external.json`), and subfolders with a `server.properties` in the `find_in` folders of `agent.toml`. |
| POST | `/v1/servers` | Create a server: `{name, exe, port, pack}`. A new folder gets a `pumpkin.toml` with that port, offline mode, telemetry off and no Bedrock listener. |
| PUT | `/v1/servers/<name>/port` | `{"port": 25570}` sets the Java port for the next start. Stop the server before changing it. |
| DELETE | `/v1/servers/<name>` | Permanently removes a stopped managed server's folder, backups and backup schedule. Shared builds and other servers' backups are retained. |
| POST | `/v1/servers/<name>/start`, `/stop`, `/restart` | Starting on an occupied Java port returns 409. The message appears as `start_error` on the stopped server. `stop` sends `stop` on the console and waits; kill only after a timeout. |
| GET | `/v1/servers/<name>/stats` | Now: state, RSS, CPU, uptime, and `health` (MSPT mean and max over the last 100 ticks, TPS, tick rate, players) for pack servers. Planned: `?window=60` time series with p95/p99, chunks loaded, network bytes. |
| WS | `/v1/servers/<name>/console` | Log lines out (with the last 500 on connect), console commands in. |
| POST | `/v1/builds/<pack>-<key>` | Upload a pack build: a zip of a `moddedmc-pack` output folder (up to 1 GiB). An id that is already there returns 200 at once. |
| GET | `/v1/builds` | Uploaded builds: id, pack, version. |
| POST | `/v1/servers/<name>/deploy` | `{build}`: a stopped server runs that build from now on, and its `mods/*.mmc` are replaced with the build's. |
| GET | `/v1/servers/<name>/mods` | Drop-in mods in `mods/`: file, id and version from `mod.json`, size. |
| PUT | `/v1/servers/<name>/mods/<file>`, DELETE likewise | Add (an `.mmc` with a `mod.json`, up to 256 MiB) or remove a drop-in mod. Stopped servers only. |
| POST | `/v1/servers/<name>/backups`, GET likewise | Take and list backups of the server folder (all of it but `logs/`, `stats.json`, `session.lock` and dotfiles). Files are cut into content-defined chunks (FastCDC, about 64 KiB) stored once each, so a backup costs only what changed. A running server gets `save-off`, `save-all flush` (the agent waits for "Saved the game") and `save-on` around it. |
| POST | `/v1/servers/<name>/backups/<id>/restore` | Stopped servers only: the folder goes back to that backup, chunk hashes checked, files it didn't have removed. |
| DELETE | `/v1/servers/<name>/backups/<id>` | Removes the backup and any chunks no other backup uses. |
| PUT | `/v1/servers/<name>/backups/schedule`, GET likewise | `{"every_minutes": 360, "keep": 8}` or `null` (off). While the server runs, the agent backs it up every interval and keeps the newest `keep` automatic backups (`"scheduled": true` in the list). Backups taken by hand are never removed by retention. |
| POST | `/v1/servers/<name>/pregen` | Start pregeneration on a running server whose pack has `port-chunky`: `{radius, center_x, center_z, circle}`. 409 when the server doesn't answer `chunky start` (no port, or a task is running). |
| GET | `/v1/servers/<name>/pregen` | Progress read from the port's `[Chunky]` lines: state (`idle`, `running`, `paused`, `finished`, `cancelled`), world, chunks, percent, ETA, chunks per second. |
| POST | `/v1/servers/<name>/pregen/pause`, `/continue`, `/cancel` | Control the running task. |

Stats come from the server itself: pack servers started with `MODDEDMC_STATS=1` write `stats.json` every two seconds (`engine/moddedmc-host/src/live_stats.rs`). The agent never polls with game commands.

## Install

The app installs the agent:

- Local: the desktop app starts its installed moddedMC agent. The Windows installer includes the signed Playit daemon, which starts when needed for a tunnel. Playit still requires the user's sign-in and approval.
- Over SSH (`app/src-tauri/src/install.rs`, offered when Add device can't reach an agent): it reads `uname -sm`, copies the agent built for that target (`aarch64-unknown-linux-gnu` on the Pi; from `MODDEDMC_AGENT_DIR/<triple>/` or the checkout's `target/<triple>/release/`), writes `agent.toml` with the owner set to the app user's Tailscale login unless one exists, runs `loginctl enable-linger` so it runs without a login session, and enables the `moddedmc-agent` systemd user service. Linux only. The script was run under WSL with systemd stubbed; it hasn't run against a real Pi yet.

Nothing else is installed on the device. Pack builds are made on the laptop and pushed.
