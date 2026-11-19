# Roadmap

This file is the plan for the whole project and the decisions behind it. Every agent and person working in the devmogged repos reads it before starting. Tasks live in [BACKLOG.md](BACKLOG.md). If a decision here is wrong, change it here first, then change the code.

## Goal

Modded Minecraft that plays exactly like the original mods on vanilla 26.3, runs much faster, uses far less RAM, disk and bandwidth, and is easy to host and join. The server is Rust (our Pumpkin fork). Mods are Rust ports. Players use the official Java client, with an optional lean companion mod when a pack needs it.

## Decisions (2026-11-18)

| Area | Decision |
| --- | --- |
| Java mods | Not run. kiln (baking Forge) is dead. Every mod is a Rust port. |
| Version | Minecraft Java 26.3, following Pumpkin forward. Port each mod's **latest** version, whatever loader or MC version that is. |
| Engine | `devmogged/Pumpkin`, our fork. We change anything we need. Commit often, merge `upstream` often, keep big new systems in new crates so merges stay small. Generic fixes go upstream as PRs **opened by a human** (Pumpkin-MC bans agent-opened PRs). |
| Vanilla parity | Full parity, built by us from the decompiled 26.3 server (Vineflower). Pumpkin's missing systems are ours to finish. |
| Mods | All Rust mods, including [ports](PORTS.md), ship as drop-in `.mmc` files. The [stable ABI](MOD-ABI.md) lets them request named, versioned interfaces and run across server builds. Packs use the stock server for each target with a set of mod files. |
| Removing a mod | Its blocks, items and entities are deleted (Forge behavior). The app takes an automatic snapshot before every rebuild. |
| Client | Official 26.3 client. Ported content is server-side plus an auto-sent resource pack wherever vanilla can show it. When a pack needs more (Create kinetics, contraptions, animated rigs, HUD, keybinds), the pack enables the **companion**: one Fabric 26.3 mod, data-driven, built once. Mods never ship client code. They describe models, animations, GUIs and renderers as data the server sends. No from-scratch client. |
| Bandwidth | Built for slow uplinks (the Pi on a home connection). Assets are content-hashed and fetched once per pack version, not per join, and can arrive through the join exe or any file transfer. With the companion: client chunk cache by hash, zstd with a trained dictionary instead of zlib, rate shaping, entity updates only for what is in view. Plain vanilla clients still work. |
| Joining | Normal offline-mode server (`online-mode=false`), no login plugin. Skins: default, Mojang's public API, or the player's choice. A join exe sets up the companion profile in whatever launcher it finds (official, Prism, MultiMC, TLauncher, SKLauncher), with a zip and instructions as the fallback. |
| World storage | Compact native format (evolving Pumpkin's `pump`), content-addressed chunk store with deduplicated incremental backups, lossless Anvil import and export so any vanilla tool can open the world. Skipping storage of untouched chunks (regenerate from seed) only after worldgen is proven bit-exact on x86 and ARM. |
| Pregen | A Chunky port that uses every core, writes straight into the compact format, and runs on the Pi or the laptop (the app offers both). |
| App | Rust desktop app on Tauri 2 + React (same stack as Plaid's launcher). Inspired by T3 Code and Plaid, not a copy. Devices: local, LAN over SSH, or Tailscale. It finds installed and running servers, shows stats and console, builds packs, deploys, backs up and pregens. |
| Device control | Only the owner. The device agent listens on loopback and tailnet addresses, trusts the owner's Tailscale identity (`tailscale whois`), and LAN control goes over SSH. No pairing tokens. |
| Repos | `devmogged/Pumpkin` (engine), `devmogged/moddedMC-rust` (platform, ports in progress on per-mod branches), `devmogged/mods` (public, finished ports whose license allows it), `devmogged/mods-private` (the full master list, including restricted assets). |
| Licenses | A port keeps the original's code license. Assets under All Rights Reserved (Create, Saints Dragons, Cataclysm) live only in `mods-private`. Code goes public where its license allows. |
| Done | A system or port is done when the differential harness matches vanilla 26.3 (or the original mod on its loader) and the performance gates pass. Compiling and unit tests are not done. |

## Targets

v1 gate, raised as we measure:

- Raspberry Pi 5 (8 GB), a buja-sized pack, 10 players: 20 TPS, MSPT p99 under 25 ms, server RSS under 1 GB.
- World on disk at least 5x smaller than Anvil for the same chunks.
- Client with companion in a big pack: 2 to 3 GB RAM (Forge today: 10 GB+).
- Join over a 2 Mbps uplink: playable after the one-time asset download, with no stalls walking around.

## Architecture

```
devmogged/Pumpkin          engine: vanilla parity, world, protocol, hooks for moddedmc
moddedMC-rust
  api/                     stable C ABI and SDK for drop-in mods (.mmc)
  engine/                  engine workspace: Pumpkin path deps, profiles
  mods/*                   drop-in mods and ports
  engine/packs/dev         stock server with Pumpkin and the host
  engine/moddedmc-host     content host inside the server: drop-in loader, registries, ticks, persistence
  engine/moddedmc-bot      headless client for scenarios
  harness/                 differential harness: same scenario on vanilla and ours, diff the results
  packer/                  packs the stock server and mod files for each target
  agent/                   device agent: installs, runs, monitors servers on a device
  app/                     desktop app (Tauri 2 + React)
  companion/               Fabric 26.3 client companion (data-driven renderers, chunk cache, zstd)
  join/                    join exe for players
```

### Ordering

Engine work follows what the buja pack needs first: worldgen and datapacks, then core survival (mobs, loot, crafting, villagers), then redstone, then the rest. Ports run in parallel with engine work so the API is shaped by real mods.

First ports, in order: Create core (kinetics, contraptions, fluids, trains), Saints Dragons (in parallel: rideable flying entities, GeckoLib-style animated models), L_Ender's Cataclysm, Legendary Monsters, then the Create addons from buja. Then the rest of buja, then anything else.

## How we work

- Read this file, [BACKLOG.md](BACKLOG.md) and [ARCHITECTURE.md](ARCHITECTURE.md) first.
- Take a task in BACKLOG.md by putting your name and branch on it. One task per branch.
- Vanilla behavior comes from the decompiled 26.3 server, never memory or the wiki. Mod behavior comes from the mod's own source (or decompiled jar if there is none).
- Every gameplay change gets a harness scenario. Every performance claim gets before and after numbers on the same machine.
- Run fmt, clippy and tests locally before pushing. Commit messages: conventional commits with a scope, subject line only.
- Never commit restricted assets to a public repo.
- The work does not stop at milestones. When a task is done, take the next one.
