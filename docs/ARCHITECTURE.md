# Architecture

moddedMC-rust is Pumpkin (a Rust Minecraft Java server) plus a content host. Mods are native libraries in the server's `mods/` folder that talk to the host through a stable C ABI, so a mod built once keeps working when the server is rebuilt or updated. Players join with a stock 26.3 client; all mod gameplay runs on the server.

## Layout

| Path | What | License |
| --- | --- | --- |
| `engine/pumpkin` | Pumpkin, git submodule pinned to an upstream commit. Never edited in git; our changes are `patches/pumpkin.patch`. | GPL-3.0 |
| `patches/pumpkin.patch` | The engine patch stack (see below). `scripts/engine.ps1 apply\|save\|build`. | GPL-3.0 |
| `engine/moddedmc-host` | Content host linked into the server: mod loader, host side of the ABI, registry, persistence, batched ticks, block events. | GPL-3.0 |
| `engine/moddedmc-bot` | Headless Java client for scenario tests (join flow from Pumpkin's `pumpkin-tester`). | GPL-3.0 |
| `api/` | `moddedmc-api`: the stable C ABI (`api/src/abi.rs`) and the Rust SDK mods use. No server code inside. | MIT OR Apache-2.0 |
| `mods/*` | Rust mods (`cdylib`), packed to `.mmc` by `scripts/pack-mod.ps1`. | MIT OR Apache-2.0 |
| `recompiler/` | Fabric JAR converter: `fabric-shim` (Java), `fabric-host` (the ABI mod that runs TeaVM output), `build-mod.ps1`. | MIT OR Apache-2.0 |
| `host/` | `moddedmc`, the host app (server folder + tunnel). | MIT OR Apache-2.0 |
| `scripts/` | `engine.ps1`, `pack-mod.ps1`, `scenario.ps1` (console + bot test runner), `host-check.ps1`, `boot-measure.ps1`. | |
| `tests/scenarios`, `tests/bench`, `tests/oracle` | Scenario files for acceptance tests, benchmarks and the Fabric oracle. | |

There are two Cargo workspaces. `engine/pumpkin` (with `engine/moddedmc-*` as members) builds the server with Pumpkin's compiler, lockfile, profile and lints. The root workspace builds everything that only talks to the server through the ABI: `api`, `mods/*`, `recompiler/fabric-host` and `host`. Nothing in the root workspace has to match the server's build.

## Engine patch

1. **Wiring** (`crates/pumpkin`): `moddedmc_host::attach(&server)` before plugins load; the workspace lists our crates and uses `api/` by path.
2. **`/forceload` fix**: forced chunks also take a chunk-load ticket (upstream only marked them active, so they never loaded). Upstreamable as is.
3. **Dialog serialization** (`pumpkin-protocol/src/java/client/dialog.rs`): plugin-built Java dialogs are written in vanilla's network form (field names read from the 26.3 `net.minecraft.server.dialog` classes). Upstream returned an error that `try_send_packet` dropped. Inputs and item bodies are still left out.
4. **Custom data write-through** (`World::set_block_entity_custom_data`): a write also updates the chunk's saved block-entity NBT and marks the chunk dirty. Upstream copies live block entities back into the chunk only in `save_block_entities`, and on player-leave unload the chunk has often already been written and dropped by then, so changes were lost. The same race still affects vanilla block-entity data (e.g. barrel contents); that is an upstream issue to report.
5. **Client information** (`pumpkin-protocol`): reads `particle_status`, which 1.21.2+ clients send.
6. **Modded block states** (`pumpkin-data/src/modded.rs`): `moddedmc_host::load_mods()` runs before the server loads any world, and the blocks mods define get state ids after vanilla's. The state lookups (`BlockState::from_id`, `Block::from_state_id`, `Block::from_name`) fall back to that table, so the rest of Pumpkin handles a modded state like a vanilla one: its physics are copied from its vanilla look-alike, and it has no Pumpkin block behaviour. Anvil chunks save modded states by name (`shaft:shaft` with its properties). Pumpkin's `.pump` format stores numeric ids, so for modded states it stores a saved id instead: an index into `config/moddedmc/states.txt`, an append-only list of state names that the host writes before any world loads. Every packet that writes block states to a client (chunk data, block update, multi-block update) writes `BlockStateId::to_client()`, the look-alike.

## Mods

The full contract is [MOD-ABI.md](MOD-ABI.md). In short:

- **Loading.** At startup the host reads `mods/` (relative to the server folder). A `.mmc` is a zip of `mod.json` and one library per platform; the host extracts the one for this machine to `mods/.cache/<name>-<hash>.<ext>` and loads it. A `.jar` is converted once by `recompiler/build-mod.ps1` into the same cache. A bare `.dll`/`.so`/`.dylib` is loaded as is. A bad mod is logged and skipped; the others still load.
- **Handshake.** A library exports `MODDEDMC_ABI` (the major version), `MODDEDMC_ID` and `moddedmc_mod`. The host checks the version, creates `config/<id>/`, and calls `moddedmc_mod` with a table of host functions; it gets back the mod's table of callbacks.
- **Boundary.** Only `#[repr(C)]` values and `extern "C"` functions cross. Strings and bytes are borrowed for one call; bytes going back are copied through a sink. Neither side frees the other's memory, so each library can use its own Rust version, std and allocator.
- **Players** are opaque handles valid for one call. Mods act through host functions (`give_block`, `teleport`, `show_menu`, ...), which run server code, so there is no question of which copy of Tokio or `tracing` a mod is using.
- **Panics.** The SDK wraps every callback in `catch_unwind`, logs the panic through the host and returns a neutral result. Unwinding across `extern "C"` would abort the server.
- **Unloading.** Libraries stay loaded until the process exits.

## Content host

- **Registration.** On the first server tick the host calls each mod's `register` with a staging registry; blocks, block entities and commands are committed only if it returns success. Commands are root literals open to every player; the mod gets the whole command line.
- **Identity and state.** A custom block is a vanilla stand-in block with a block entity (barrel, dropper, dispenser). The host stores `moddedmc:{id, v, s}` (block id, state layout version, state bytes) as custom data on the stand-in's block entity, which Pumpkin already saves with the chunk. An ordinary barrel has no such data and stays vanilla.
- **Items.** A placing item is the stand-in item with custom data `moddedmc:block = "<id>"`. Breaking a custom block drops that item instead of the vanilla drop.
- **Ticks.** The host indexes live custom block entities by type and by a stable per-position phase (`phase(pos, interval)`), so a tick visits only the entities due on it and equal-interval entities spread over their interval. Due entities go to their mod in one `tick` call per (mod, world); the mod hands back only the states it changed. Oversized states are discarded. State saved under another layout version is not handed to the mod (fail closed).
- **Interaction, place and break.** Right-clicking a custom block cancels the stand-in's own interaction and calls `use_block`; the returned message goes to the action bar. After a placed stand-in is tagged, `place_block` may replace the initial state; before removal, `break_block` gets the final state. There is no player for console placements.
- **Menus.** `show_menu` sends a vanilla multi-action dialog. A click arrives as a `DialogClickActionEvent`; the host passes it to every mod's `menu_click` and each acts only on its own action ids.
- **Lazy custom data.** Pumpkin loads a block entity's custom data only when the entity is woken, so the host wakes it (`get_block_entity`) before reading a block's identity.
- **Discovery.** Every 20 ticks the index is rebuilt from the custom data of live block entities, which also picks up chunks loaded since.
- **`blocks/1` blocks** are real block states (engine patch 6), not stand-ins. At startup the host asks each mod for its blocks (`define_blocks`) and installs them all at once. Their item is the look-alike's item tagged `moddedmc:block = "<id>"`; when one is placed, the host turns the placed look-alike into the modded state that looks like it on the next tick (so facing carries over) and sends the mod a `PLACE` event. Use and break go to the owning mod's `block_event`; breaking drops the item.
- **Removed mods.** `config/moddedmc/blocks.txt` lists every `blocks/1` block the server has had (name, properties, look-alikes). A listed block that no mod defines any more is installed anyway, with no owner: its blocks stay in the world, saved and shown as before, and come back to life when the mod does.

## Testing

`scripts/scenario.ps1` starts the server, drives it from the console and from `moddedmc-bot`, and checks log lines. Console-only host commands `moddedmc place|dump` place content and print saved state. `tests/scenarios/m1-counter.txt` and `m2-wayshrine.txt` are the M1 and M2 acceptance tests; `tests/oracle/sethome.txt` runs the same Fabric scenario on a real Fabric server and on ours. The bot can also press dialog buttons (`click <action-id>`) and prints dialogs it receives.

## Known limits

- Mods are trusted code with full access to the process. There is no sandbox and no permission prompt.
- The ABI covers what the current mods need (blocks with state, commands, menus, teleport, player info). A mod that needs more waits for the ABI to grow; additions are appended, so existing mods keep working.
- `.jar` conversion needs the converter toolchain on the server machine (JDK, Gradle, clang, cargo; Windows PowerShell script) and takes minutes on the first start with a new jar.
- `pack-mod.ps1` packs only the library for the machine it runs on; other platforms are added to the zip by hand for now.
- Libraries are never unloaded, so mods cannot be hot-reloaded.
- Stand-ins must have a block entity; blocks without one need an engine-side identity store.
- No resource pack delivery yet: custom blocks look like their stand-in or look-alike, and custom items show only a custom name.
- A `blocks/1` block has its look-alike's shape, collision, hardness and light, because a stock client predicts those from what it sees. Other packets that carry a state id (block particles, falling blocks) still send the modded id, which a stock client treats as air: shortcut until a mod needs them.
- Modded state ids are numbered at startup and differ between runs and servers; worlds store names. At most 65,535 states in all (vanilla uses 35,723).
- Discovery is O(custom blocks) once a second.
