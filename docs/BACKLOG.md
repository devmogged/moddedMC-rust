# Backlog

Pick a task, write your name and branch next to it, and move it to In progress. When it's done, move it to Done with a one-line result (numbers if there are any). Decisions behind these tasks are in [ROADMAP.md](ROADMAP.md).

Status: `todo`, `doing (<who>, <branch>)`, `done`.

## P0: foundations

Everything else depends on these.

| # | Task | Status |
| --- | --- | --- |
| F1 | Engine patch moved into commits on `devmogged/Pumpkin` (branch `moddedmc`); `engine/` is its own workspace with fast `pack` profile; compiled-in ports and pack servers (`engine/ports`, `engine/packs`) next to drop-in mods. | done: host or port change rebuilds the server in 5 s (STATUS.md) |
| F2 | Differential harness (spec: [HARNESS.md](HARNESS.md)): run one scenario file against a real vanilla 26.3 server and against ours with the same seed and bot inputs, then diff block state, block entities, entities, inventories and selected packets. Report mismatches by position and field. | todo |
| F3 | Pack builder (spec: [PACKS.md](PACKS.md)): a `pack.toml` (mods with versions, target, config) becomes one server binary per target. Mods are linked in statically, not loaded as `.dll`. Cross-compile `aarch64-unknown-linux-gnu` for the Pi from Windows (cargo-zigbuild or cross). Cache builds. | todo |
| F4 | Content model v1 per [CONTENT.md](CONTENT.md): first-class modded registries in the engine, per-world id map, vanilla-view translation with a generated resource pack, then the companion view. Exposed to compiled-in ports directly and to drop-in mods through ABI additions where it fits. | todo |
| F5 | Perf and RAM bench harness on the Pi: same seed, bot count, view distance and duration; record MSPT p50/p95/p99, RSS, disk, bytes sent per player. | todo |

## Engine parity (buja order)

| # | Task | Status |
| --- | --- | --- |
| E1 | Audit Pumpkin against vanilla 26.3: list every missing or partial system, with the vanilla classes involved. Output `docs/parity.md`. | todo |
| E2 | Datapack worldgen: load custom biomes, noise settings, density functions, features, structures from datapacks (Terralith, Tectonic, Incendium, Nullscape are datapacks). Harness against vanilla with the same datapacks. | todo |
| E3 | Worldgen determinism across x86_64 and aarch64 (no platform-dependent float paths). Byte-identical chunks on both. | todo |
| E4 | Lighting parity. | todo |
| E5 | Mobs: AI goals and brains per Pumpkin's planned design, spawning, loot. | todo |
| E6 | Villagers, trading, raids. | todo |
| E7 | Redstone with vanilla update order. | todo |
| E8 | Nether, End, portals, dragon fight. | todo |
| E9 | Enchanting, brewing, anvils, smithing. | todo |
| E10 | Advancements, statistics, remaining commands. | todo |

## World and storage

| # | Task | Status |
| --- | --- | --- |
| W1 | Measure `pump`, Anvil and Linear on a real pregenerated world (size, write speed, load speed). | todo |
| W2 | Compact format: palette packing, zstd with a trained dictionary, per-chunk content hash. | todo |
| W3 | Content-addressed chunk store with incremental, deduplicated snapshots; restore any point. | todo |
| W4 | Lossless Anvil export and import (modded content round-trips through stand-ins plus data). | todo |
| W5 | Chunky port: parallel pregen on all cores, radius or shape, pause and resume, progress to the app. | todo |
| W6 | Untouched-chunk elision (only after E3 proves determinism). | todo |

## Client companion and network

| # | Task | Status |
| --- | --- | --- |
| C1 | Companion skeleton (Fabric 26.3): handshake channel, capability flags, version check. | todo |
| C2 | Content-hashed asset delivery: pack manifest, one-time download, delta updates, offline install from the join exe. | todo |
| C3 | Client chunk cache by hash. | todo |
| C4 | zstd with trained dictionary as a negotiated packet compression. | todo |
| C5 | Data-driven renderers: block models, rotating kinetic parts, belts, moving contraptions, animated entity rigs (GeckoLib-format models and animations). | todo |
| C6 | Data-driven GUIs, HUD elements, keybinds, particles. | todo |
| C7 | Server send scheduling: nearest chunks first, uplink rate shaping, view-based entity updates. | todo |

## App, agent, join

| # | Task | Status |
| --- | --- | --- |
| A1 | Device agent ([DEVICES.md](DEVICES.md)): runs on a device, lists installed and running servers, installs pack builds, start and stop, console stream, stats (MSPT, RSS, players, disk), backups. Listens on loopback and tailnet, trusts the owner's Tailscale identity; LAN through SSH. | doing (claude): `agent/` has device info, server folders, start/stop/restart, stats (RSS, CPU, MSPT, TPS, players from the pack server's `stats.json`), console WebSocket (plain text), owner check, `pumpkin.toml` port and offline-mode seeding. Next: build upload, mods, backups, external server detection, SSH install. |
| A2 | Desktop app (Tauri 2 + React): devices, servers, mod browser, pack editor, deploy, console, stats, backups, pregen. Design with the impeccable skill. | doing (claude): `app/` has the device list (local, Tailscale, SSH tunnels), server rows with live state, tick-health bars and player counts, start/stop/restart, console, performance (RSS, CPU), Join list with server-list pings, mod index. Next: pack editor, deploy, backups, pregen, companion install for Join. |
| A3 | Join exe: detect launchers, install the companion profile and assets, add the server to the server list; zip fallback. | todo |
| A4 | Brand: name, favicon, app icon, server-list icon (owner decides the name). | todo |

## Ports

| # | Mod (latest version) | Status |
| --- | --- | --- |
| P1 | Create 6.0.10 (NeoForge 1.21.1): kinetics first, then contraptions, fluids, trains. Code MIT, assets ARR. First slice: a shaft and cogwheel network that spins, matches real Create in the harness, and deploys to the Pi from the app. | todo |
| P2 | Saints Dragons 0.9.88 (1.20.1): rideable flying dragons, GeckoLib models. Code LGPL-3.0 or MIT (check LICENSE), assets ARR. | todo |
| P3 | L_Ender's Cataclysm (latest). | todo |
| P4 | Legendary Monsters (latest). | todo |
| P5 | Create addons from buja: New Age, Create Addition, Enchantment Industry, Central Kitchen, Steam 'n' Rails, Ultimate Factory, More Recipes, Copper and Zinc, Recycle. | todo |
| P6 | Rest of buja: triage into port, already vanilla, client-only (not needed), library (not needed). | todo |

## Done

| # | Result |
| --- | --- |
| M1 | One custom block end to end (see STATUS.md). |
| M2 | Wayshrine hand port. |
| M3 | Unchanged Fabric JAR on Pumpkin via TeaVM (kept as research; 83% of mods use Mixin, so ports are the path). |
| M4 | `moddedmc` host app with playit and Tailscale tunnels. |
