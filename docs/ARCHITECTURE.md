# Architecture

moddedMC-rust is Pumpkin (a Rust Minecraft Java server) plus a content host, with mods loaded as native Rust plugins (`.dll` on Windows, `.so` on Linux). Players join with a stock 26.3 client; all mod gameplay runs on the server.

## Layout

| Path | What | License |
| --- | --- | --- |
| `engine/pumpkin` | Pumpkin, git submodule pinned to an upstream commit. Never edited in git; our changes are `patches/pumpkin.patch`. | GPL-3.0 |
| `patches/pumpkin.patch` | The engine patch stack (see below). `scripts/engine.ps1 apply|save|build`. | GPL-3.0 |
| `engine/moddedmc-api` | The content API mods compile against: types, the `ContentMod` trait, the `ContentHost` service, `export_plugin!`. | GPL-3.0 |
| `engine/moddedmc-host` | Content host linked into the server: registry, persistence, batched ticks, block events. | GPL-3.0 |
| `engine/moddedmc-bot` | Headless Java client for scenario tests (join flow from Pumpkin's `pumpkin-tester`). | GPL-3.0 |
| `mods/*` | Native mods (`cdylib`). | GPL-3.0 |
| `host/` | `moddedmc`, the host app (server folder + tunnel). | MIT OR Apache-2.0 |
| `scripts/` | `engine.ps1`, `scenario.ps1` (console + bot test runner), `host-check.ps1`, `boot-measure.ps1`. | |
| `tests/scenarios`, `tests/bench` | Scenario files for acceptance tests and benchmarks. | |

`engine/moddedmc-*` and `mods/*` are members of the Pumpkin workspace (`package.workspace`), so they build with Pumpkin's compiler, lockfile, profile and lints. That is required, not a convenience: a native mod and the server exchange plain Rust values (`Arc<Player>`, trait objects), so both must come from the same build. `scripts\engine.ps1 build` builds the server and the mods together.

## Engine patch

Kept small and generic, so most of it can go upstream:

1. **Wiring** (`crates/pumpkin`): `moddedmc_host::attach(&server)` before plugins load; the workspace lists our crates.
2. **Services** (`plugin/mod.rs`): `PluginManager::services` is public so the host can offer its service before any plugin loads.
3. **`/forceload` fix**: forced chunks also take a chunk-load ticket (upstream only marked them active, so they never loaded). Upstreamable as is.
4. **Dialog serialization** (`pumpkin-protocol/src/java/client/dialog.rs`): plugin-built Java dialogs are written in vanilla's network form (field names read from the 26.3 `net.minecraft.server.dialog` classes). Upstream returned an error that `try_send_packet` dropped. Inputs and item bodies are still left out.
5. **Custom data write-through** (`World::set_block_entity_custom_data`): a write also updates the chunk's saved block-entity NBT and marks the chunk dirty. Upstream copies live block entities back into the chunk only in `save_block_entities`, and on player-leave unload the chunk has often already been written and dropped by then, so changes were lost. The same race still affects vanilla block-entity data (e.g. barrel contents); that is an upstream issue to report.
6. **Client information** (`pumpkin-protocol`): reads `particle_status`, which 1.21.2+ clients send.

## Native mods

A mod is a `cdylib` loaded by Pumpkin's own native plugin loader (`NativePluginLoader`, on by default). `moddedmc_api::export_plugin!` exports the three symbols it reads (`PUMPKIN_API_VERSION`, `METADATA`, `plugin`). In `on_load` the mod fetches the `ContentHost` service (`moddedmc:content-host`) and calls `add_mod` with its `ContentMod`. It can also use any Pumpkin API directly: commands, events, players, worlds.

**One rule matters.** A `.dll` statically links its own copy of every library, including their globals: Tokio's runtime context, the `tracing` dispatcher, `LazyLock`s. Reading and calling plain methods on server objects is fine. Code that may spawn tasks or needs the runtime must run as server code, so the host offers those operations as function pointers on `ContentHost` (`give_block`, `teleport`, `show_dialog`). A mod that logs calls `context.init_log()` once. Memory crosses the boundary freely: the server uses the system allocator.

## Content host

- **Registration.** On the first tick after a mod is added, the host calls `register` with a staging registry; definitions are committed only if it returns `Ok`.
- **Identity and state.** A custom block is a vanilla stand-in block with a block entity (barrel, dropper, dispenser). The host stores `moddedmc:{id, v, s}` (block id, state layout version, state bytes) as custom data on the stand-in's block entity, which Pumpkin already saves with the chunk. An ordinary barrel has no such data and stays vanilla.
- **Items.** A placing item is the stand-in item with custom data `moddedmc:block = "<id>"`. Breaking a custom block drops that item instead of the vanilla drop.
- **Ticks.** The host indexes live custom block entities by type and by a stable per-position phase (`phase(pos, interval)`), so a tick visits only the entities due on it and equal-interval entities spread over their interval. Due entities go to their mod in one `tick` call per (mod, world); the mod edits states in place and marks them `changed`. Oversized states are discarded. State saved under another layout version is not handed to the mod (fail closed).
- **Interaction, place and break.** Right-clicking a custom block cancels the stand-in's own interaction and calls `use_block`; the returned message goes to the action bar. After a placed stand-in is tagged, `place_block` may replace the initial state; before removal, `break_block` gets the final state. The player is `None` for console placements.
- **Panics.** Every call into a mod goes through one helper that catches a panic, logs it and drops that call's result, so a broken mod cannot take the server down.
- **Lazy custom data.** Pumpkin loads a block entity's custom data only when the entity is woken, so the host wakes it (`get_block_entity`) before reading a block's identity.
- **Discovery.** Every 20 ticks the index is rebuilt from the custom data of live block entities, which also picks up chunks loaded since.

## Testing

`scripts/scenario.ps1` starts the server, drives it from the console and from `moddedmc-bot`, and checks log lines. Console-only host commands `moddedmc place|dump` place content and print saved state. `tests/scenarios/m1-counter.txt` and `m2-wayshrine.txt` are the M1 and M2 acceptance tests. The bot can also press dialog buttons (`click <action-id>`) and prints dialogs it receives.

## Known limits

- Mods are trusted code with full access to the process. There is no sandbox and no permission prompt.
- A mod must be rebuilt whenever the server is rebuilt; a mismatched `.dll` is undefined behavior. `PUMPKIN_API_VERSION` only catches deliberate API bumps.
- Windows cannot unload a loaded `.dll`, so mods cannot be hot-reloaded there.
- Stand-ins must have a block entity; blocks without one need an engine-side identity store.
- No resource pack delivery yet: custom blocks look like their stand-in, and custom items show only a custom name.
- Discovery is O(custom blocks) once a second.
