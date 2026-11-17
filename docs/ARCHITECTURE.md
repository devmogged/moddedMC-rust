# Architecture

moddedMC-rust is Pumpkin (a Rust Minecraft Java server) plus a content host that loads mods as WASM components. Players join with a stock 26.3 client; all mod gameplay runs on the server.

## Layout

| Path | What | License |
| --- | --- | --- |
| `engine/pumpkin` | Pumpkin, git submodule pinned to an upstream commit. Never edited in git; our changes are `patches/pumpkin.patch`. | GPL-3.0 |
| `patches/pumpkin.patch` | The engine patch stack (see below). `scripts/engine.ps1 apply|save|build`. | GPL-3.0 |
| `engine/moddedmc-host` | Content host linked into the server: registry, persistence, batched ticks, block events. | GPL-3.0 |
| `engine/moddedmc-bot` | Headless Java client for scenario tests (join flow from Pumpkin's `pumpkin-tester`). | GPL-3.0 |
| `wit/mods` | `moddedmc:mods@0.1.0`, the content API. | MIT OR Apache-2.0 |
| `mods/*` | Mods, built with `cargo build --release --target wasm32-wasip2`. | MIT OR Apache-2.0 |
| `scripts/` | `engine.ps1`, `scenario.ps1` (console + bot test runner), `boot-measure.ps1`. | |
| `tests/scenarios`, `tests/bench` | Scenario files for acceptance tests and benchmarks. | |

`engine/moddedmc-*` are members of the Pumpkin workspace (`package.workspace = "../pumpkin"`), so they build with Pumpkin's lockfile and lints. The root workspace holds only `mods/*`.

## Engine patch

Kept small and generic, so most of it can go upstream:

1. **Extension hook** (`pumpkin-wasm-host-common/src/extension.rs`, ~40 lines): an optional `HostExtension` with three function pointers: add imports to the v0.1 linker, look up extra exports after instantiation, and receive the plugin once its store runs. The v0.1 host instantiates through `InstancePre` so the raw `Instance` is available.
2. **Wiring** (`crates/pumpkin`): `moddedmc_host::install()` before the plugin runtime is created, `moddedmc_host::attach(&server)` before plugins load.
3. **`/forceload` fix**: forced chunks also take a chunk-load ticket (upstream only marked them active, so they never loaded). Upstreamable as is.

## Content mods

A content mod is an ordinary Pumpkin v0.1 plugin component that also imports `moddedmc:mods/registry` and exports `moddedmc:mods/content`. It can use the whole Pumpkin plugin API (commands, events, items) next to ours. The two sets of bindings are generated separately and merged into one component by the wasip2 linker.

- **Registration.** On the first tick after a mod loads, the host calls `content.register`. The mod's `registry.add-*` calls are staged and committed only if `register` returns `ok`; registration is closed outside that call (`registry-frozen`).
- **Identity and state.** A custom block is a vanilla stand-in block with a block entity (0.1: barrel, dropper, dispenser). The host stores `moddedmc:{id, v, s}` (block id, state layout version, state bytes) as custom data on the stand-in's block entity, which Pumpkin already saves with the chunk. An ordinary barrel has no such data and stays vanilla. No storage patch is needed.
- **Items.** A placing item is the stand-in item with custom data `moddedmc:block = "<id>"`. Breaking a custom block drops that item instead of the vanilla drop.
- **Ticks.** The host indexes live custom block entities by type and by a stable per-position phase (`phase(pos, interval)`), so a tick visits only the entities due on it and equal-interval entities spread over their interval. Each tick, due entities are sent to their mod in one `tick-block-entities` batch per (mod, dimension). Updates are validated (known, unique tokens; size limit) and applied only if the whole batch is valid; a trap discards the batch. State saved under another layout version is not handed to the mod (fail closed).
- **Interaction.** Right-clicking a custom block cancels the stand-in's own interaction and calls `use-block`; the returned message goes to the action bar.
- **Discovery.** Every 20 ticks the index is rebuilt from the custom data of live block entities, which also picks up chunks loaded since.

## Testing

`scripts/scenario.ps1` starts the server, drives it from the console and from `moddedmc-bot`, and checks log lines. Console-only host commands `moddedmc place|dump` place content and print saved state. `tests/scenarios/m1-counter.txt` is the M1 acceptance test.

## Known limits (0.1)

- Stand-ins must have a block entity; blocks without one need an engine-side identity store.
- No resource pack delivery yet: custom blocks look like their stand-in, and custom items show only a custom name.
- Pumpkin releases a chunk's block entities only when no player watches it; force-loaded chunks without players are never unloaded.
- Discovery is O(custom blocks) once a second.
