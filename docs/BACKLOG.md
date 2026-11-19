# Backlog

Pick a task, write your name and branch next to it, and move it to In progress. When it's done, move it to Done with a one-line result (numbers if there are any). Decisions behind these tasks are in [ROADMAP.md](ROADMAP.md).

Status: `todo`, `doing (<who>, <branch>)`, `done`.

## P0: foundations

Everything else depends on these.

| # | Task | Status |
| --- | --- | --- |
| F1 | Engine patch moved into commits on `devmogged/Pumpkin` (branch `moddedmc`). The engine has its own workspace and the fast `pack` profile. | done. See [STATUS.md](STATUS.md) for build times. |
| F2 | Differential harness (spec: [HARNESS.md](HARNESS.md)): run one scenario file against a real vanilla 26.3 server and against ours with the same seed and bot inputs, then diff block state, block entities, entities, inventories and selected packets. Report mismatches by position and field. | todo |
| F3 | Pack builder (spec [PACKS.md](PACKS.md)). Build the stock server binary for each target and ship the pack's mods as `.mmc` files. Cross-compile for Pi and cache builds. | doing. `moddedmc-pack build` handles local Rust mods and prebuilt files, writes `pack.lock` and caches build outputs. Next are index and git sources and a real Pi test. |
| F4 | Content model v1 per [CONTENT.md](CONTENT.md). | Step 1: frozen custom blocks, items and persistent block entities, append-only world IDs, Java carrier and item-model translation, locally generated packs, and versioned ABI tables. The Create bot rig exercises the stock protocol. A `.mmc` also ships the mod’s `data/` (recipes, loot tables, tags, loaded as built-in datapack `mod/<id>`) and its own `assets/`. Datapack tags reach gameplay tag checks for blocks and items, so tools mine mod blocks and recipes match mod items. `scripts/new-mod.ps1` scaffolds a mod that builds and passes its scenario as is. Companion view and the remaining registries follow. |
| F5 | Perf and RAM bench harness on the Pi: same seed, bot count, view distance and duration; record MSPT p50/p95/p99, RSS, disk, bytes sent per player. | todo |
| F6 | Drop-in `.mmc` ports through named, versioned ABI 1 interfaces. | done. Chunky loads before worlds, preserves progress logs and resumes from its shutdown checkpoint. Windows, seed 12345, `pack` profile, three-run medians (compiled in to drop-in). Radius 1000 (16,129 chunks) went from 28.55 s / 602 MiB to 26.30 s / 587 MiB. Radius 500 (4,225 chunks) went from 7.67 s / 476 MiB to 7.65 s / 450 MiB. Reproduce the drop-in runs with `scripts/pregen-bench.ps1 -Radius 1000 -Runs 3 -SkipJava` and repeat at radius 500. Next is the [Create interface slice](INTERFACES-PLAN.md). |

## Engine parity (buja order)

| # | Task | Status |
| --- | --- | --- |
| E1 | Audit Pumpkin against vanilla 26.3: list every missing or partial system, with the vanilla classes involved. Output `docs/parity.md`. | done (first pass, breadth): [parity.md](parity.md). Nearly every vanilla command, entity type, structure, feature and screen exists; `sequence` and `template` features now place (desert wells, deep dark and ancient city sculk, sulfur pools and springs). Fidelity is F2's job. |
| E2 | Datapack worldgen: load custom biomes, noise settings, density functions, features, structures from datapacks (Terralith, Tectonic, Incendium, Nullscape are datapacks). Harness against vanilla with the same datapacks. | todo |
| E3 | Worldgen determinism across x86_64 and aarch64 (no platform-dependent float paths). Byte-identical chunks on both. | todo |
| E4 | Lighting parity. | todo |
| E5 | Mobs: AI goals and brains per Pumpkin's planned design, spawning, loot. | todo |
| E6 | Villagers, trading, raids. | todo |
| E7 | Redstone with vanilla update order. | todo |
| E8 | Nether, End, portals, dragon fight. | todo |
| E9 | Enchanting, brewing, anvils, smithing. | todo |
| E10 | Advancements, statistics, remaining commands. | todo |
| E11 | Faster chunk generation (the Chunky port and every player wait on it). Generation is CPU bound and scales: 2,601 chunks take 44.3 s on one generation thread and 4.9 s on 16 (9x); all 32 hardware threads add only 12% (SMT). Per chunk, one thread: noise 5.9 ms, surface 2.7 ms, features 2.1 ms, lighting 0.9 ms, carvers 0.5 ms, structure references 0.29 ms, biomes 0.27 ms. With 16 threads structure references takes 2.1 ms (7x), so it contends on a lock; the others slow 1.3 to 1.4x (SMT and memory). Surface is slow because it also evaluates the ore-vein density volume for the whole chunk (vanilla does veins inside the noise pass), so density functions are about 8.6 of the 12.9 ms. Targets, biggest first: density-function evaluation for noise and veins (batch or SIMD evaluation, shared interpolation between neighbours), then the structure-references wait (threads block on a shared `OnceLock` while one computes a jigsaw start; about 8% at 16 threads). Report chunks/s on the laptop and the Pi before and after, checked against the vanilla chunk dumps in `pumpkin-world`. | todo |

## World and storage

| # | Task | Status |
| --- | --- | --- |
| W1 | Measure `pump`, Anvil and Linear on a real pregenerated world (size, write speed, load speed). | todo. First number: 16,129 chunks pregenerated with seed 12345 take 376 MB as `pump` against 197 MB of vanilla Anvil. |
| W2 | Compact format: palette packing, zstd with a trained dictionary, per-chunk content hash. | todo |
| W3 | Content-addressed chunk store with incremental, deduplicated snapshots; restore any point. | doing (claude): the agent's backups are a content-addressed store of FastCDC chunks of whole files, format-agnostic. Next: measure dedup on a real pregenerated world (W1), and cut by chunk instead of by bytes if region rewrites defeat it. |
| W4 | Lossless Anvil export and import (modded content round-trips through stand-ins plus data). | todo |
| W5 | Chunky port: parallel pregen on all cores, radius or shape, pause and resume, progress to the app. | doing (claude): `mods/chunky` (`packs/pregen`): `/chunky center\|radius\|shape square\|circle\|start\|pause\|continue\|cancel\|progress`, Chunky's selection and progress lines. Historical compiled-in results against Fabric 26.3 + Chunky 1.5.66 (-Xmx4G), seed 12345, laptop (16 cores, 32 threads), `pack` profile: radius 500 (4,225 chunks) 7.4 s / 495 MB peak vs 32.8 s / 2,674 MB; radius 1000 (16,129 chunks) 29.7 s / 589 MB vs 105.6 s / 2,263 MB, so 3.6 to 4.4 times faster at a quarter of the RAM (Java runs vary: 26.6 to 32.8 s at radius 500). See F6 for the drop-in comparison. Vanilla feature-generation parity is tracked in E2. The agent drives it (`/v1/servers/<name>/pregen`) and the app has a Pregen tab with live progress. A stopping server saves the task, and `/chunky continue` resumes it after a restart. Next: Pi numbers (E11 is parked: the owner wants everything working first). |
| W6 | Untouched-chunk elision (only after E3 proves determinism). | todo |

## Client companion and network

| # | Task | Status |
| --- | --- | --- |
| C1 | Companion skeleton (Fabric 26.3): handshake channel, capability flags, version check. | implemented: [protocol 1](COMPANION.md), tested with a headless bot and codec tests. Real-client check pending. |
| C2 | Content-hashed asset delivery: pack manifest, one-time download, delta updates, offline install from Join. | implemented: manifest and verified hash cache tested with Create and offline cache tests. Real-client pack reload pending. |
| C3 | Client chunk cache by hash. | todo |
| C4 | zstd with trained dictionary as a negotiated packet compression. | todo |
| C5 | Data-driven renderers: block models, rotating kinetic parts, belts, moving contraptions, animated entity rigs (GeckoLib-format models and animations). | todo |
| C6 | Data-driven GUIs, HUD elements, keybinds, particles. | todo |
| C7 | Server send scheduling: nearest chunks first, uplink rate shaping, view-based entity updates. | todo |

## App, agent, join

| # | Task | Status |
| --- | --- | --- |
| A1 | Device agent ([DEVICES.md](DEVICES.md)): runs on a device, lists installed and running servers, installs pack builds, start and stop, console stream, stats (MSPT, RSS, players, disk), backups. Listens on loopback and tailnet, trusts the owner's Tailscale identity; LAN through SSH. | doing (claude): `agent/` has device info, server folders, start/stop/restart, stats (RSS, CPU, MSPT, TPS, players from the pack server's `stats.json`), console WebSocket (plain text), owner check, `pumpkin.toml` port and offline-mode seeding, build upload and deploy, external (not agent-started) servers, running or stopped (remembered folders and `find_in`), SSH install from the app, per-server drop-in mods, backups (FastCDC chunks in a shared store, restore, delete with GC; a running server saves first), scheduled backups with retention (automatic backups only; manual ones stay). Next: a real-Pi install test. |
| A2 | Desktop app (Tauri 2 + React): devices, servers, mod browser, pack editor, deploy, console, stats, backups, pregen. Design with the impeccable skill. | doing: `app/` has the device list (local, Tailscale, SSH tunnels), server rows with live state, tick-health bars and player counts, start/stop/restart, console, performance (RSS, CPU), Join list with server-list pings, mod index, deploy a build folder, a Packs section (add a `pack.toml`, build each target with `moddedmc-pack` and its streamed output, deploy the build to a stopped server on any device), uploaded builds per device, a Mods tab per server (add or remove `.mmc` files while stopped), a Backups tab (back up, restore, remove, automatic backups: off, hourly, every 6 hours or daily, and how many to keep), a Pregen tab (radius, center, shape, live progress, pause, cancel). UI1 adds icon-derived server colours, live telemetry, performance charts, a pregeneration view, Ctrl+K and completion notifications. Device RAM usage is measured. CPU load and disk usage require more agent telemetry. Packs can be edited: add a `.mmc` or a mod crate folder, remove a mod (the `pack.toml` keeps its comments). Next: companion install for Join. |
| A3 | Join in the app: detect launchers, install the companion profile and assets, add the server to the server list, export a ZIP if needed. | implemented: official launcher, Prism and MultiMC layouts, scratch backup and retry tests, and headless Edge mock. Real launcher play test pending. |
| A4 | Brand: name, favicon, app icon, server-list icon (owner decides the name). | todo |

## Ports

| # | Mod (latest version) | Status |
| --- | --- | --- |
| P1 | Create: kinetics, contraptions, fluids and trains. MIT code, reserved assets. | P1a: [Create .mmc](../mods/create) has shaft, small and large cogs and a creative motor. The bot verified placement, signed 2:1 and 1:1 gear transmission, a 24 RPM setting surviving restart, and stopping after a break. Andesite alloy, the shaft, cogwheel and alloy recipes, kinetic block loot tables and mineable tags load from the Create 6.0.10 (1.21.1) jar’s data (`tests/scenarios/create-data.txt`). In a no-player rig with 256 shafts and 24 checked transitions, median active process CPU across three runs was 0.234 s on moddedMC 26.3 and 0.844 s on Java Create 6.0.8 (Minecraft 1.20.1); re-measure against 6.0.10 on 1.21.1, the latest release. Stock-client visual capture is open. P1b: belts, more transmitters, stress and destructive conflict parity. |
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
