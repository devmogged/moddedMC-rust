# Status

## M0 — base proven (2026-11-17)

### Pins

| What | Value |
| --- | --- |
| Pumpkin | upstream `87f8822ac36da62102350977be2f62f155a303dc` (2026-11-16), submodule `engine/pumpkin`; since M1 plus `patches/pumpkin.patch` |
| Pumpkin version | `0.2.2+26.3-26.63` |
| Minecraft Java | 26.3, protocol 777 |
| Vanilla server jar (build-time data) | SHA-1 `33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c`, from Mojang, kept in ignored `cache/`, passed via `PUMPKIN_MINECRAFT_SERVER_JAR` |
| Rust | 1.100.0 stable, targets `x86_64-pc-windows-msvc`, `wasm32-wasip2` |
| Wasm runtime | Wasmtime 50.0.1 (component model, WasmGC and exceptions enabled by Pumpkin) |
| Plugin WIT | `pumpkin:plugin@0.1.0` (v0.2 exists behind the `plugin-v0_2` feature, unfinished) |
| Licenses | Pumpkin server GPL-3.0; `pumpkin-plugin-api`, `-wit`, `-utils` MIT OR Apache-2.0 |

### Verified

- `cargo build --locked --release -p pumpkin` builds (17 min, fat LTO).
- `mods/hello` builds to a 524 KB `wasm32-wasip2` component, loads (`Loaded hello (0.1.0)`), and its `/hello` command runs from the console.
- **Not yet verified:** a join from a stock 26.3 client. Joins are tested with `moddedmc-bot`, a protocol-level headless client (see M1).

### Resource baseline

`scripts/boot-measure.ps1`, existing world, no players, 30 s idle window, telemetry off, other settings default (Bedrock listener on). 20-core Windows 11 desktop.

| Run | Ready (s) | RSS (MB) | Private (MB) | Peak RSS (MB) | Idle CPU (% of machine) |
| --- | --- | --- | --- | --- | --- |
| Stock Pumpkin | 0.39–0.40 | 31.8–32.2 | 15.7–16.2 | 32.3 | 0.11 |
| + `hello.wasm`, first load (compiles component) | 0.72 | 46.1 | 25.3 | 166.5 | 0.12 |
| + `hello.wasm`, cached | 0.39–0.48 | 36.4–36.5 | 18.6 | 36.5 | 0.06–0.11 |

"Ready" is time to Pumpkin's `Started server` log line. These numbers are idle-only; tick-time p95/p99 under load come with M1.

### Notes

- Pumpkin sends telemetry heartbeats by default (`[telemetry] enabled = true` in `pumpkin.toml`). Our run dirs turn it off; the host app should default it off and ask.
- Unsigned plugins log a warning on load.

## M1 — one custom block end to end (2026-11-18)

`demo:counter` (`mods/counter`) is registered through `moddedmc:mods@0.1.0` (`wit/mods`), rendered as a vanilla barrel, and ticks once a second through the content host (`engine/moddedmc-host`). Design: [ARCHITECTURE.md](ARCHITECTURE.md).

### Verified

`scripts\scenario.ps1 -Scenario tests\scenarios\m1-counter.txt -RunDir run\m1 -Fresh` passes on the release build. A headless player (`moddedmc-bot`):

- gets a counter item from `/counter` and places it; an ordinary barrel placed next to it stays vanilla (one live counter);
- sees it tick (state 5 after ~5 s) and right-clicks it: the action bar shows `Counter: 6` instead of the barrel opening;
- teleports 8,000 blocks away: the chunk unloads (0 live counters), and on return the counter resumes from its saved count (10) without ticking while unloaded;
- after a server restart the counter keeps its identity and count (14);
- breaking it drops the custom item and removes it from the host. No traps, invalid batches or panics.

**Not verified with a real client:** the barrel look, the action-bar text and the dropped item's name as a stock 26.3 client shows them.

### Engine patch size

`patches/pumpkin.patch`, 13 KB over 9 files (including `Cargo.lock`): the WASM host extension hook (~40 lines, new file), its use in the v0.1 host and runtime (~30 lines), wiring in `main.rs` (2 lines), workspace membership, and a `/forceload` fix (forced chunks take a load ticket; upstream only marked them active, so they never loaded). Identity and state needed no storage patch: they live in block-entity custom data that Pumpkin already saves. The patch is small enough to maintain downstream; the hook and the `/forceload` fix are upstream candidates.

### Benchmark: 10,000 blocks

`tests/bench/m1-*.txt`, release builds, fresh world, one `moddedmc-bot` player at (1074, 120, 1074) holding the same 9x9 chunks (view distance 4), 10,000 blocks in a 100x100 grid at y=100. Tick percentiles from `/tick query` (last 100 ticks, three samples) and `MODDEDMC_TICK_STATS=200`; memory and CPU over a 20 s window. Same 20-core Windows 11 desktop.

| Run | Tick p50 (ms) | p95 (ms) | p99 (ms) | RSS (MB) | Private (MB) | CPU (% of machine) |
| --- | --- | --- | --- | --- | --- | --- |
| Stock Pumpkin, 10,000 vanilla barrels | 4.9–5.5 | 9.1–10.7 | 11.3–11.5 | 317 | 289 | 1.51 |
| Patched, no mods, 10,000 vanilla barrels | 5.0–5.6 | 7.5–8.4 | 9.3–12.2 | 298 | 256 | 1.25 |
| Patched, 10,000 `demo:counter` | 6.3–6.4 | 10.4–10.9 | 14.4–15.8 | 318 | 286 | 1.59 |

- The patch costs nothing measurable when no content mod is loaded (the rows differ within run-to-run noise).
- 10,000 counters at one tick per second each (500 due per tick, spread by per-position phase) add about 1–1.5 ms to the median tick, 2 % of the 50 ms budget.
- Host stage cost per tick at 500 entries: index rebuild 0.39–0.44 ms (once a second, averaged), building the batch (reading NBT state) 0.78–0.81 ms, the component call (copy in, guest, copy out) 0.38–0.40 ms, writing states back 0.43–0.45 ms. The WASM boundary is ~0.8 us per entity; most of the cost is host-side NBT access.
- Before phasing, all 10,000 ticked on the same tick: p95 was ~19 ms. Phasing removed that spike.
- `moddedmc place` of 10,000 blocks in one tick takes ~11 s (block updates for every block). It is a console benchmark tool, not a gameplay path.

### Next optimisations (measured, not yet done)

1. Keep live state in the host index and write NBT only when a chunk saves, instead of reading and writing NBT per entity per tick (~1.2 ms of the ~2 ms).
2. Maintain the index incrementally on chunk load/unload instead of rebuilding it once a second.

## M2 — a playable hand port: Wayshrines (2026-11-18)

`mods/wayshrine` (`wayshrine:shrine`, barrel stand-in, event-driven, no ticks) is an original Waystones-style mod. Placing a shrine records it and discovers it for the placer. Using a shrine discovers it and opens a Java dialog listing every other shrine that player has discovered; picking one teleports them on top of it. Breaking a shrine removes it from every menu. Shrines and discoveries persist in the mod's data folder (`shrines.txt`). `/wayshrine` gives the item.

### Verified (headless bot, release build)

`tests/scenarios/m2-wayshrine.txt` passes: place two shrines, the menu (a vanilla-format `minecraft:multi_action` dialog received by the bot) lists the other shrine, clicking it teleports the player to `1041.5 101 1025.5`, a forged click for an undiscovered shrine is refused, and after a restart the menu still lists the saved shrine, a broken shrine leaves the menu. `m1-counter.txt` still passes.

Not verified: a real 26.3 client rendering the dialog. The dialog NBT uses field names read from the 26.3 server classes, but only the client can confirm it renders.

### Host and engine changes this milestone

- WIT: `place-block` (may replace the initial state) and `break-block` (final state). All export calls share one trap-handling helper.
- Engine: dialogs built by plugins are now serialized (upstream silently dropped them); custom block-entity data is written through to the chunk's saved NBT, fixing loss on player-leave unload (traced to the chunk being dropped before `save_block_entities`). The same upstream race still loses vanilla block-entity changes; to be reported upstream.
- Host: wakes a block entity before reading its identity (Pumpkin loads custom data lazily).
- Engine patch: 22 KB, 11 files.

### Benchmark after write-through (10,000 counters, one bot)

p50 5.3–5.6 ms, p95 10.6–11.0 ms, p99 11.5–12.6 ms; RSS 330 MB; CPU 0.86%. No regression against M1 (p50 6.3–6.4 ms).

## M3 — first Fabric JAR on Pumpkin (2026-11-18)

### Verified (headless bot, release build)

- `tests/oracle/sethome.txt` passes on a real Fabric 26.3 server (loader 0.20.1, Fabric API 0.166.2+26.3) with `sethome-1.0.0.jar`.
- The same scenario passes on Pumpkin with that unchanged JAR, recompiled by `recompiler/build-mod.ps1` (TeaVM 0.16 C backend, wasi-sdk 34, linked into `recompiler/fabric-host`). It covers no home, set, teleport home, the cooldown (8 s left on both servers), and the home surviving a restart.
- Not verified with a real 26.3 client.

### Size and startup

`sethome.wasm` is 2.0 MB. The first server start takes 2242 ms, mostly Wasmtime compiling the component once. With the module cache warm it takes 171 ms. For comparison, M1 counter takes 905 ms and 66 ms.
### Corpus

Top 500 server-capable Fabric 26.3 mods are frozen in `recompiler/corpus/top500.tsv`. Of the 455 with code, 378 use Mixin, 60 depend on another mod, and 17 are self-contained (the shim's target). Details: `recompiler/corpus/report.md`.

## M4 — host app with a tunnel (2026-11-18)

`host/` builds `moddedmc` (300 KB, one dependency: serde_json). `moddedmc run` creates the folder with telemetry off, starts Pumpkin with the console passed through, and with `--tunnel playit` finds the account's Minecraft Java tunnel to port 25565, prints its address and runs the playit agent (0.15.26) until the server stops.

### Verified (`scripts\host-check.ps1`, release build)

- `-Tunnel none`: the bot joins on 127.0.0.1, `stop` typed into `moddedmc` stops Pumpkin, exit 0. Server ready about 0.3 s after launch.
- `-Tunnel playit`: passed once in four runs made within a few minutes. In the pass, the bot joined through the public playit address (about 16 s), the server logged the join, and `stop` also ended the agent. In the other three, the edge accepted the TCP connection but the agent never got it; the agent logs showed playit API rate limits (`TooManyRequests`) during these runs. A run after a 10-minute cool-down passed: joined through the tunnel, clean stop, no leftover agent. So restarting the agent repeatedly within minutes is the failure mode, not the tunnel itself. The agent uses 18–50 MB working set.
- playit's shared Minecraft tunnels route on the host name in the handshake, so a client must dial the tunnel's domain. The bot now sends the host it dialed, as a real client does (it sent `127.0.0.1` before).
- Not verified: Tailscale (not signed in on this machine) and a real 26.3 client.

### Notes

- Repeated agent restarts get playit API rate limits (`TooManyRequests`); a second agent on the same key steals the tunnel. The check kills leftover processes when it fails.
- On Windows, killing `moddedmc` itself does not stop Pumpkin; Ctrl+C reaches both because they share the console.

## Native mods (2026-11-18)

Mods are now native Rust plugins (`.dll`/`.so`) loaded by Pumpkin's own native loader, instead of WASM components under Wasmtime. Design and rules: [ARCHITECTURE.md](ARCHITECTURE.md). The WIT content API, the WASM extension hooks in the engine patch (now 18 KB) and the wasi-sdk path of the recompiler are gone.

### Verified (release build)

- `tests/scenarios/m1-counter.txt` and `m2-wayshrine.txt` pass with `counter.dll` and `wayshrine.dll`.
- The unchanged `sethome-1.0.0.jar`, recompiled by TeaVM to C, compiled with the system clang and linked into `fabric-host`, passes `tests/oracle/sethome.txt` as `sethome.dll`. Server start with it loaded: 84–179 ms, with no compile step (WASM: 2.2 s first start, 171 ms cached).

### Benchmark: 10,000 counters (`tests/bench/m1-counters.txt`)

The baseline was re-measured in the same session: this machine ran slower than for the M1 numbers (no-mods p50 was 5.0–5.6 ms then).

| Run (same session) | Tick p50 (ms) | p95 (ms) | p99 (ms) | RSS (MB) |
| --- | --- | --- | --- | --- |
| No mods, 10,000 vanilla barrels | 4.6–6.9 | 6.5–11.3 | 8.6–13.0 | 305 |
| Native, 10,000 `demo:counter` | 8.0–9.9 | 12.5–14.7 | 15.9–17.9 | 303–337 |

Host stages per tick at 500 due entries: mod call 0.04–0.05 ms (WASM was 0.38–0.40 ms, so the boundary is about 8x cheaper), batch collection (reading NBT state) 0.82–0.93 ms, writing state back 2.2–2.5 ms, index rebuild 0.41–0.45 ms (once a second, averaged). The plugin boundary is no longer the cost: writing state through to chunk NBT is (the M2 write-through fix). Keeping state in host memory and writing NBT only when the chunk saves is the next speed-up.

### Costs

- A release mod `.dll` is 41–43 MB: each carries its own optimized copy of Pumpkin's core. Linking one takes about 5 minutes (fat LTO).
- Mods are trusted, unsandboxed code, and must be rebuilt whenever the server is.

## Drop-in mods: stable mod ABI (2026-11-18)

Mods no longer have to come from the server's build. They talk to the host through a C ABI (`api/src/abi.rs`, [MOD-ABI.md](MOD-ABI.md)) and are loaded by the host from the server's `mods/` folder: `.mmc` packages (zip of `mod.json` plus one library per platform), Fabric `.jar` files (converted on the first start and cached in `mods/.cache/`), or bare libraries. This replaces the native-plugin setup above, whose costs (41 MB mods, rebuild with every server build) are gone. Pumpkin's own plugin loader and `plugins/` are untouched; our patch no longer needs `PluginManager::services` public (patch now 16.6 KB).

The ABI design was reviewed by GPT-6.2 Sol before it was built. Adopted: player, command and registry handles are bound to the calling thread (not `Send`), teleport reports "accepted", old `plugins/<id>` data moves to `config/<id>`, the converted-jar cache is keyed by the server version, and the published struct layout is pinned by tests that CI runs on Windows, Linux and macOS. Deferred: host functions do not catch panics of their own (a panic in server code called by a mod still unwinds to the `extern "C"` boundary and aborts), menu clicks go to every mod rather than to the mod that showed the menu, and tick results are written without rechecking that the block was not replaced during the call.

### Verified (headless bot, release build)

- **M1** (`tests/scenarios/m1-counter.txt`) passes with `target/mods/counter.mmc`.
- **M2** (`tests/scenarios/m2-wayshrine.txt`) passes with `target/mods/wayshrine.mmc`.
- **M3 drop-in**: the unchanged `sethome-1.0.0.jar` copied into `mods/` is converted on the first start and passes `tests/oracle/sethome.txt`, restart included.

### Size and startup

| | Before (Rust-typed plugins) | Now |
| --- | --- | --- |
| Mod library | 41–43 MB | 112 KB (counter), 202 KB (wayshrine), 1.5 MB (sethome incl. its Java runtime) |
| Packed mod | | 58 KB `counter.mmc`, 97 KB `wayshrine.mmc` |
| Building one mod | ~5 min link, needs the server's build | ~4 s, any Rust toolchain |
| Loading mods at start | | 67 ms first start (extracts), 2.3 ms after |
| A new Fabric jar | manual `build-mod.ps1`, ~5 min | automatic on first start, 9.6–10.8 s; 61–76 ms after |
| Server start with mods | 84–179 ms | 81–162 ms |

The 10,000-counter benchmark was not rerun after the ABI change; the per-tick mod call now copies changed states through a sink instead of editing `Vec`s in place.

### Repository

`CONTRIBUTING.md`, `AGENTS.md` (with `CLAUDE.md` pointing to it), issue forms, a PR template, `SECURITY.md`, a triage playbook for agents, CI for the root workspace on three OSes, and label automation (area, size, `abi`, needs-rebase, needs more info, PR limits in dry-run until `PR_LIMITS_ENFORCE` is set), modeled on the T3 Code repository.


## Raspberry Pi cross build (2026-11-19)

The server and mods build for the Pi 5 (`aarch64-unknown-linux-gnu`, glibc 2.36 as on Raspberry Pi OS bookworm) on Windows, with zig as the C compiler and linker for `ring`. `pip install ziglang cargo-zigbuild`, put `ziglang`'s folder on PATH, then:

- Server, from `engine/pumpkin`: `cargo-zigbuild build --release -p pumpkin --target aarch64-unknown-linux-gnu.2.36`.
- Mods: `scripts\pack-mod.ps1 -Crate <name> -Pi` adds `linux-aarch64.so` to the `.mmc`.

### Verified (QEMU user mode on x86_64, not a Pi)

- The ARM64 server starts ("Started server; took 806 ms" with no mods, 882 ms with one; about 157 MB resident including QEMU).
- It loads `pillar.mmc` from its `linux-aarch64.so` (37 ms); the bot joins and `/pillar` and `/pillarstruct` work.
- All 10 scenarios pass on ARM64 at normal speed: `scripts\scenario.ps1 -Exe <wrapper>`, where the wrapper is a `.cmd` running `wsl.exe -- qemu-aarch64 -L <arm64 glibc root> <ARM64 pumpkin>` (mods packed with `-Pi`). Getting there fixed two things that a slow server exposed:
  - Engine: Pumpkin kicked a player whose teleport confirmation answered an older teleport ("Wrong teleport id"), e.g. `/tp` sent before the client confirmed its spawn. Vanilla ignores those confirmations; so does the engine now. On a slow server or a laggy connection this kicked real players.
  - Scenarios: `expect` waits up to 10 s for its line, and the first world edit after a teleport uses `cmduntil` (resend until the area is loaded and the command takes) instead of a fixed wait.
- Not measured: startup, tick time and memory on a real Pi 5.

## Stock 26.3 client (2026-11-19)

The vanilla 26.3 client (offline, launched straight from Mojang's jars and libraries in `cache/`, joining with `--quickPlayMultiplayer`) against the release server with frost, wisp, backpack, pillar and counter, then shaft:

- It joins through the configuration phase with the modded `frost:glacier` biome in its registries.
- `/glacier` repaints the area: grass and foliage take the biome's colors (`#b8e8f0`, `#a0d8e8`).
- `/smelter` opens the furnace-style machine screen titled "Smelter" with its progress arrow.
- The mods' resource pack is offered, and with the server marked to accept packs the client downloads and applies it: its resource manager loads `server/.../d58f98d2-...`, the SHA-1 the server announced.
- Found and fixed: vanilla commands rejected a modded block's properties (`/setblock shaft:shaft[axis=y]`: "does not have property 'axis'"). The command parser now resolves modded states by name (engine patch); `tests/scenarios/resource-pack.txt` checks it.
- Not seen yet: the shaft block and its item name in the client. The client also sometimes stalls while loading resources before it connects (3 of 10 launches), with nothing logged.
