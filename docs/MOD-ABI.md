# Mods

## For server owners

Put mod files in your server folder's `mods/` and start the server. That is all.

```
myserver/
  mods/
    wayshrine.mmc     a Rust mod
    sethome.jar       an unchanged Fabric mod
```

- **`.mmc`**: a moddedMC mod. One file holds builds for every platform the author packed; the server picks the one for your machine. It loads in milliseconds.
- **`.jar`**: a Fabric mod. The first start with a new jar converts it to native code, which takes a few minutes and needs the converter tools (see [recompiler/README.md](../recompiler/README.md)). The result is cached in `mods/.cache/`, so later starts are instant. Most Fabric mods use Mixin and cannot be converted yet; the server logs why a jar was skipped.
- To remove a mod, delete its file and restart. `mods/.cache/` can be deleted at any time; it is rebuilt.
- A mod's own files live in `config/<mod id>/`. Fabric mods use `config/` as on a Fabric server.
- The server log shows each mod it loaded (`moddedmc: loaded wayshrine 0.2.0 from mods\wayshrine.mmc`) and why any mod was skipped. One broken mod never stops the others or the server.

The mod ABI is **1**. A mod can run on any ABI 1 server that provides its required interfaces. Mods are trusted code: only install mods you trust, as with any Minecraft server plugin.

## For mod authors (Rust)

A mod is a `cdylib` crate that depends on `moddedmc-api` (in `api/`) and implements `moddedmc_api::Mod`.

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
moddedmc-api = { path = "../../api" }
```

```rust
use moddedmc_api::{Command, Mod, RegistrationError, Registry};

struct Hello;

impl Mod for Hello {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("hello", "Says hello", 0)
    }

    fn command(&self, _tag: u64, command: Command<'_>) -> i32 {
        command.reply("Hello!");
        1
    }
}

moddedmc_api::export_mod!(Hello);
```

Build and pack it:

```powershell
scripts\pack-mod.ps1 -Crate hello    # -> target\mods\hello.mmc, ready for mods/
```

What the SDK offers:

| | |
| --- | --- |
| `Mod::start` | Before worlds load. Require your interfaces here. |
| `Mod::register` | Once, on the first server tick: `add_block_entity`, `add_block`, `add_command`. |
| `Mod::tick` | Due block entities of one dimension; `set_state(index, bytes)` saves a new state. |
| `Mod::use_block`, `place_block`, `break_block` | A player used, placed or broke one of your blocks. |
| `Mod::command` | One of your commands ran. `reply` answers the player or the console. |
| `Mod::menu_click` | A player picked a button of a menu (`Player::show_menu`). Every mod sees every click: check the action id is yours. |
| `Player` | `name`, `dimension`, `position`, `send_message`, `action_bar`, `give_block`, `teleport`, `show_menu`. Valid only during the call that handed it to you; store the name, not the player. |
| free functions | `info`, `warn`, `error` (server log), `data_dir()` (`config/<id>/`), `tick_count()`, `dimensions()`. |

The companion's standalone pack screens also use `Player::show_menu` and
`Mod::menu_click` without changing their ABI. The reserved
`moddedmc:pack_screen` marker and its JSON tooltip are documented in
[Companion standalone screens](COMPANION.md#standalone-pack-screens-version-2).
Treat all callbacks as untrusted input and validate ownership and live state.

Rules:

- The mod id is the package name (or `export_mod!(M, id = "...")`): lowercase letters, digits, `_`, `-`, `.`. It names the data folder. Block ids are `namespace:path` and may use any namespace except `minecraft`.
- Calls can come from several threads at once, so the mod type must be `Send + Sync`.
- A panic is caught by the SDK, logged, and treated as "did nothing". It never crashes the server.
- Block entity state is opaque bytes, at most 64 KiB, saved with the chunk. Bump `state_version` when its layout changes; the server will not hand old-layout state to the new code.
- You do not need the server's source, compiler version or build to build a mod. Any Rust toolchain that builds `api/` works.

`mods/counter` (a ticking block) and `mods/wayshrine` (blocks, a menu, teleport, saved data) are complete examples.

## The ABI

The contract between the server and a mod library is plain C: `#[repr(C)]` structs and `extern "C"` functions, defined in [`api/src/abi.rs`](../api/src/abi.rs). Any language that can build a shared library with C exports can implement it; the Rust SDK is one wrapper over it.

A mod library exports three symbols:

| Symbol | Type | |
| --- | --- | --- |
| `MODDEDMC_ABI` | `u32` | Major ABI version. The server refuses a library whose version differs. |
| `MODDEDMC_ID` | `Str` | The mod id. Read before the mod starts, so the server can create its data folder. |
| `moddedmc_mod` | `extern "C" fn(&'static HostApi) -> *const ModApi` | Starts the mod. Returns null to refuse (wrong host, failed start). |

`HostApi` is the server's table of functions (log, registration, player actions, ticks, dimensions); `ModApi` is the mod's table of callbacks. Both live until the process exits.

Ownership and lifetimes:

- `Str` is a borrowed pointer and length, valid only for the call it is passed in, unless documented as static (mod id and version, data folder, dimension names).
- A `PlayerRef` is valid only during the call that passed it. Null means "no player".
- Bytes going back to the server (new states, messages) go through a `Sink`; the server copies them before `put` returns.
- Neither side frees memory the other allocated. Each library has its own allocator and standard library; that is fine because nothing allocated crosses.

Versioning:

- `ABI_VERSION` changes only for breaking changes (a changed signature, a removed function, different meaning). Old mods then need a rebuild.
- New host functions and new mod callbacks are appended to the end of their table, and `size` (the table's byte size) grows. The SDK reads the original ABI 1 prefix on any ABI 1 host. A mod that requires an unavailable extension refuses to start.

## Interfaces

`HostApi::get_interface(name, version)` returns a process-lifetime table for the exact namespaced name and version, or null. Each table is `#[repr(C)]`, begins with its byte size (`usize`), and uses C ABI functions. Append fields without changing existing offsets or behavior. A breaking change gets a new interface version, and the host can serve both versions.

The Rust SDK checks the host's size before reading `get_interface` and checks the returned table's size before using its functions. Require interfaces in `Mod::start` to refuse startup with a log line naming the missing interface.

### Version 1 tables

| Interface | Function | Contract |
| --- | --- | --- |
| `moddedmc:world` | `dimension(index)` | Static dimension name, or empty past the end. |
| | `command_dimension(source)` | Static dimension name for the current command source. Empty when the source has no world. |
| `moddedmc:chunks` | `is_generated(dimension, x, z, ctx, completed)` | Check whether a chunk is loaded or saved. The callback receives the result. |
| | `ensure_generated(dimension, x, z, ctx, completed)` | Skip an existing chunk, otherwise load or generate it. The callback receives true after the operation returns, or false if interrupted. |
| `moddedmc:commands` | `add(reg, name, description, op_level, nodes, count)` | Stage a root and its children during registration. The 1 to 128 child nodes name an earlier parent or the root. Nodes are literals, bounded integers or (with the appended extension) quotable strings and single/multiple entity/player selectors. Executable nodes pass a tag to `ModApi::command`. The permission is `<mod id>:command.<name>`, with a default operator level from 0 to 4. |
| | `integer(source, name, out)` | Copy a parsed integer to the caller's output slot. False means the argument was absent. |
| | `feedback(source, text)` | Send command feedback using the source's feedback settings. |
| | `string(source, name, sink)` | Copy a parsed quotable string at sink key 0; false if absent. |
| | `select(source, name, kind, ctx, visit)` | Resolve the selector with Pumpkin's entity/player parser for the specified kind; visit each resolved entity with its world, borrowed entity handle and display name. False on resolution failure; sends the command's native error. |
| | `online_player(source, name, ctx, visit)` | Resolve an online player by name or UUID across dimensions using the same synchronous visitor. False if absent. |
| | `failure(source, text)` | Send a command error to its source. |
| `moddedmc:lifecycle` | `on_stopping(reg, callback)` | Stage one callback per mod. The host runs it on a blocking thread and waits for it before world save. |
| `moddedmc:worldgen_lookup` | `biomes(world, out)` | Enumerate biome identifiers from the active dimension generator, including its datapack biome source. Does not load chunks. |
| | `noise_biomes(world, positions, count, out)` | Sample up to 256 quart-coordinate positions; the sink key is the input index. False on invalid input or unsupported generator. |
| | `height_bounds(world, bottom, top)` | Dimension's minimum block Y and exclusive maximum block Y. |
| `moddedmc:worldgen_structures` | `structures(world, out)` | Enumerate locatable structures with enabled placements in the current generator. |
| | `placements(world, target, out)` | Sink key is the placement index; value is a little-endian signed 32-bit spacing in chunks (zero for concentric rings). Indices refer to the active registry. |
| | `spread_candidate(world, placement, x, z, out)` | Resolve a random-spread placement candidate from a chunk position. The output is a chunk position, not a confirmed structure start. |
| | `ring_chunks(world, placement, out)` | Return 1 and emit indexed pairs of little-endian signed 32-bit chunk coordinates, 2 while calculation is pending, or -1 if unavailable. |
| | `structure_start(world, placement, target, x, z, out)` | Check a candidate's actual generator-derived structure start. 1 means found with block coordinates, 0 absent, 2 pending, -1 unavailable. |
| `moddedmc:container_loot` | `generate(world, player, table, seed, pos, size, out)` | Generate an independent 1–54 slot loot inventory from the active loot table registry, including player luck and chest position. Emit only occupied slots, keyed by slot index, as named-root stack NBT. Does not consume a vanilla block entity's loot table or mutate an inventory. |
| `moddedmc:player_stack_data` | `compare(player, item, expected, new, replace)` | Match exactly one single-item stack by item id and nonempty expected mod data in the main inventory or offhand. Check only, or replace and synchronize that same stack. Absent or ambiguous matches fail. |
| | `compare_lodestone(player, item, expected, new, dimension, pos)` | Atomically replace matching stack data and its untracked lodestone target. Empty dimension clears tracking. |

Chunk requests take chunk coordinates and return immediately. A false return means the request was rejected with no callback. For an accepted request the host returns the opaque context exactly once, possibly before submission returns, on a server runtime thread. Keep callbacks short. Chunky uses a coordinator thread per active dimension to plan work, receive callbacks, log progress and save task files. The server stops accepting chunk requests on shutdown and drains accepted work before it saves worlds.

The Rust wrappers live in `world`, `chunks`, `commands` and `lifecycle`. Logging and the mod data folder still use the ABI 1 base table. [Chunky](../mods/chunky) keeps its `port-chunky` id so existing task files migrate from `plugins/port-chunky` to `config/port-chunky`.

## Packaging

A `.mmc` is a zip with

- `mod.json`: `{"id": "wayshrine", "version": "0.2.0", "abi": 1}`
- `data/`, optional: the mod’s recipes, loot tables, tags, advancements and functions in datapack layout. The server loads it with every world as the built-in datapack `mod/<id>`, before the world’s own datapacks.
- `assets/`, optional: the mod’s models and textures, sent to players in the server’s resource pack. A port of a mod whose assets aren’t free to redistribute leaves them out; the server then reads them from a local copy of the original jar (`MODDEDMC_ASSET_JARS`, or a `cache/` folder above the server directory).
- one library per platform, named `<os>-<arch>.<ext>`: `windows-x86_64.dll`, `windows-aarch64.dll`, `linux-x86_64.so`, `linux-aarch64.so`, `macos-aarch64.dylib`, `macos-x86_64.dylib`.

`scripts/pack-mod.ps1` writes the manifest and the build for the current machine. Builds for other platforms (from CI or cross-compiling) go into the same zip under their names.

Built-in mod datapacks can also add world generation features through `data/<namespace>/neoforge/biome_modifier/*.json`. The `neoforge:add_features` modifier takes biome IDs or a biome tag, placed feature IDs, and a generation step. It appends features to the selected biomes at world startup. Create uses this to generate its registered zinc ores in new Overworld chunks.

To use the vanilla wall placement and neighbor rules, register the complete wall state set with matching vanilla wall physics templates. An opaque wall can use one `*_wall_post` model in every state and leave the rotating model empty. The host generates a multipart item model at `<namespace>:moddedmc/wall/<block_path>` and keeps the matching stock wall state for collision and water. The item model reads the post from custom model data flag 0 and the north, east, south and west arms from strings 0 through 3. The arm values are `none`, `low` and `tall`. A moving contraption can display that same item model with its captured selectors.

### Real content tables

| Interface | Function | Contract |
| --- | --- | --- |
| `moddedmc:content` v1 | `add_block(reg, definition)` | Stage the block, its item, its block entity name and 1 to 1,024 ordered states. The first state is the default. States provide exact property strings, a vanilla physics template, optional collision and outline boxes in sixteenths, model resource locations, and a display orientation. |
| | `set_block_material(reg, block, material)` | Copy a vanilla block's hardness, blast resistance, place and step sounds, note instrument, tool requirement, and map color to a staged content block. Geometry and waterlogging come from its physical states. The definition can override hardness and set a named vanilla map color. Appended: check `Content::require_block_material()`. |
| | `set_comparator_output(reg, selector, strength)` | Assign a fixed analog output (0 to 15) to an exact state of a block already staged by this mod. Zero is a readable signal. Unknown states and duplicate assignments are rejected. Appended: require with `Content::require_comparator_output()`. |
| | `state(selector)` | Resolve a block name or exact `namespace:path[key=value,...]` after registration. Missing definitions return `UNLOADED_STATE`. |
| | `on_tick(reg, callback)` | Call the mod once per loaded dimension in the server tick with queued state, neighbor and chunk lifecycle changes. The callback and its world handle are synchronous. |
| | `on_replaced(reg, callback)` | Report replacement of an owned block by another block type after the state write and before the old block entity is removed. The callback can copy the old entity NBT and must defer world writes to the mod tick. Moved replacements have their own event reason. Appended. |
| | `MOVED_STATE_CHANGED` | Event reason for a replacement made with `UPDATE_MOVED`, so a mod can detach a moved block (vanilla `isMoving`) instead of treating it as broken. |
| `moddedmc:items` v1 | `add_item(reg, definition)` | Stage an item with no block: its id, display name, the vanilla item it copies components from (stack size, food, tools), and its model. Stock clients see that vanilla item drawn with the model. Ids share one namespace with blocks. |
| | `item_durability(reg, item, max_damage)` | Give a registered standalone item a single-item stack limit and damage components beginning at zero. Appended to `moddedmc:items` v1. |
| `moddedmc:block_world` v1 | `read(world, positions, out, count)` | Read at most 4,096 positions without loading chunks. Unloaded positions return `UNLOADED_STATE`. |
| | `dimension_id(world)` | Canonical dimension registry ID for the callback world, such as `minecraft:the_nether`. Require `WorldExtension::Dimension`. |
| | `write(world, updates, count)` | Validate the batch before applying edits in input order. Each edit explicitly chooses neighbor and client notifications. `UPDATE_MOVED` is vanilla's move-by-piston and suppress-drops write: the replaced block entity is removed without side effects, so a container captured with `read_block_entity` doesn't scatter. Older hosts reject a batch using it. |
| | `write_with_neighbor_shapes(world, updates, count)` | Like `write`. For an entry with `UPDATE_FROM_NEIGHBOR_SHAPES`, resolve the new state's shape against the world just before applying that entry. Earlier edits in the batch are visible to the shape update. Require `WorldExtension::NeighborShapes`. |
| | `command_world(source)` | Borrow the command source's world for the callback. |
| | `read_entity`, `write_entity` | Copy Java named-root NBT for a loaded custom block entity. The write keeps its registered id and position and updates the chunk's saved copy. Writes have a 64 KiB limit. |
| | `rotate(world, position, axis, rpm)` | Set a rotating model's signed RPM around its positive X, Y or Z axis. |
| | `has_neighbor_signal(world, position)` | Vanilla `Level.hasNeighborSignal`. False when unloaded. Appended after the first release: require `WorldExtension::NeighborSignal`. |
| | `best_neighbor_signal(world, position)` | Vanilla `Level.getBestNeighborSignal`, with strength from 0 to 15. Returns 0 when unloaded. Appended: require `WorldExtension::BestNeighborSignal`. |
| | `destroy(world, position, drop)` | Vanilla `Level.destroyBlock`: break particles, loot when `drop`, then air or the block's water. Appended: require `WorldExtension::Destroy`. |
| | `game_time(world, out)` | Appended: the persisted overworld `getGameTime` / world-age tick clock, shared across dimensions. Continues while daylight cycle is disabled and is not changed by `/time set`. Returns false if the world handle is unavailable. Require `WorldExtension::GameTime` on servers supporting this entry. For refresh/decay deadlines, use this clock rather than `day_time` or `tick_count()`; saving a deadline does not by itself authorize destructive block replacement without an identity-guarded transaction. |
| | `compare_replace_content_entity(world, pos, expected_state, incarnation, expected, destination, replacement)` | Appended: identity-bound no-drop replacement of a loaded custom block and its block entity. Requires the exact state, process-local incarnation and full Java named-root NBT snapshot to still match. Destination must be a registered block state; a destination with a block entity needs full saved NBT of that entity ID and the exact site coordinates, while an entityless destination requires empty replacement NBT. A successful swap closes any open container screens and sends ordinary state/neighbor notifications after world locks are released. The chunk's state and pending entity NBT are updated together. Failure changes nothing. Require `WorldExtension::ContentEntityReplacement` and obtain the source with `World::entity_snapshot`; use `World::compare_replace_entity` for the operation. **This produces no drops**, and does not make an independent item-transfer/saved-data transaction crash-atomic. |
| | `face_sturdy(world, position, face)` | Vanilla `BlockState.isFaceSturdy`. False when unloaded. Appended: require `WorldExtension::FaceSturdy`. |
| | `collision_shape(world, position, out, capacity)` | Vanilla `BlockState.getCollisionShape(level, pos)` with an empty context, as f64 boxes relative to the block (random offset included); returns the box count, -1 when unloaded. Appended: require `WorldExtension::CollisionShape`. |
| | `describe_state(state, out)` | The full `namespace:block[key=value,...]` selector `Content::state` reads back, every property included. False for an unknown state. Appended: require `WorldExtension::DescribeState`. |
| | `fluid_id(world, position, out)` | Vanilla `FluidState.getType()` registry id, the source id for waterlogged and aquatic blocks, `minecraft:empty` for none. False when unloaded. Appended: require `WorldExtension::FluidId`. |
| | `read_block_entity(world, position, out)` | Vanilla `saveWithFullMetadata` for any block entity: id, position and fields as Java NBT. False when there is none or the position is unloaded. Appended: require `WorldExtension::BlockEntities`. |
| | `restore_block_entity(world, position, nbt)` | Replaces the entity of an already placed destination block with saved NBT, at the destination position. The saved id must match the entity the destination block made; at most `MAX_BLOCK_ENTITY_BYTES`. Marks the chunk dirty and sends clients the data. False when unloaded or mismatched. |
| | `block_drops(world, position, tool, out)` | Vanilla `Block.getDrops(state, level, pos, blockEntity, null, tool)`: one saved stack per sink call, without breaking the block or spawning anything. Empty `tool` is an empty hand. False when unloaded or the tool is unreadable. Appended: require `WorldExtension::BlockDrops`. |
| | `day_time(world, out)` | `DayTime { ticks, fixed_time }`: the overworld clock's total ticks (26.3 `Level.getOverworldClockTime()`, the day time every dimension shared in 1.21.1) and `DimensionType.hasFixedTime()`, which in vanilla holds exactly for the dimensions 1.21.1 called not `natural`. Appended: require `WorldExtension::Survival` (this row and the next three). |
| | `destroy_speed(world, position, out)` | Vanilla `BlockState.getDestroySpeed`: the loaded state's hardness, negative when unbreakable. False when unloaded. |
| | `can_survive(world, position, state)` | Vanilla `BlockState.canSurvive` for `state` as if at `position`, without changing the world (crops include their light check). 1, 0, or -1 for an unknown state or unloaded position. |
| | `block_item(state, out)` | Vanilla `Block.asItem()` as a namespaced id, `minecraft:air` when the block has none. False for an unknown state. |
| `moddedmc:fluid_world` v1 | `inspect(world, position, cell, fluid)` | Read a loaded block's runtime state and source, replaceability and waterlogging flags. Hosts with tick control also report whether the cell is a liquid block and whether it blocks motion. The sink receives the still-fluid id when the actual fluid state contains fluid. Loaded dry cells succeed without calling the sink. |
| | `schedule_tick(world, position, fluid, delay)` | Schedule a normal-priority vanilla fluid tick at a loaded position. Accepts a registered non-empty fluid id and a delay of at most 255 ticks, or `DEFAULT_FLUID_TICK_DELAY` for the fluid's usual delay in that dimension. |
| | `would_regenerate_source(world, position, fluid)` | Predict source conversion for a hypothetical flowing liquid at the position. Uses the world's gamerules, horizontal collision faces and the supporting block. Returns 1 or 0, or -1 if the required state is unavailable. |
| | `clear_ticks(world, position)` | Clear pending fluid ticks at a loaded position. The appended entry also indicates that block-world writes accept `UPDATE_KNOWN_SHAPE`, which skips neighbor shape updates independently of neighbor and client notifications. Require both with `fluid_world::require_tick_control()`. |
| | `velocity(world, position, out)` | Read the flow vector at a loaded vanilla fluid cell. A dry cell gives zero. The appended entry is available through `fluid_world::require_flow()`. |
| | `water_evaporates(world, position)` | Read the world's 26.3 water evaporation setting for a loaded position. |
| `moddedmc:entities` v1 | `has_scoreboard_tag(entity, tag)` | Appended scoreboard tag membership check. Require `EntitiesExtension::ScoreboardTags` before relying on the result. |
| `moddedmc:entities` v1 | `players(world, out, capacity)` | Appended. The ids of the world's players, spectators and dead ones included; returns the total, which can exceed `capacity`. Require `EntitiesExtension::Players`. |
| `moddedmc:entities` v1 | `age(EntityRef) -> i32` | Appended at byte 248. Vanilla entity tick count, including item entities; -1 for an invalid entity. The SDK returns `None` on older hosts. |
| `moddedmc:entities` v1 | `on_entity_inside(reg, callback)` | Stage one callback per mod, called like vanilla `Block.entityInside` for entities touching one of the mod's blocks. It gets the world, position, state and a borrowed entity, all valid only during the call. |
| | `view(entity, out)` | Fill kind bits (player, living, item), on-ground flag, position, motion, yaw, pitch, head yaw and body yaw. |
| | `type_id(entity)` | The entity type's resource name. |
| | `change(entity, change)` | Apply the masked fields: motion (resent to clients, a player's own client included, like vanilla `hurtMarked`), yaw, head yaw, body yaw, on-ground. Non-finite motion is rejected. |
| | `id`, `with_entity(world, id, ctx, visit)` | The movement extension, appended to the table (`EntitiesExtension::Movement`). An entity's id, and a borrowed entity for a loaded id, valid during `visit`. |
| | `flags`, `body` | Alive, sneaking, flying, hanging (`HangingEntity`) and push-ignored (`IGNORE_ENTITY` piston reaction) bits; the collision box and movement input, which for a player comes from the keys their client last sent. |
| | `move_by`, `set_position`, `reset_fall_distance` | Vanilla `Entity.move(SELF, delta)` with block collisions and stepping, `setPos` and `fallDistance = 0`. Pumpkin's move skips players, whose client moves them. |
| | `add_effect`, `step_height`, `set_step_height` | Vanilla `addEffect` by effect id, and the `minecraft:step_height` base value. |
| | `entities_in(world, min, max, except, out, capacity)` | Ids of the entities and players whose boxes intersect the box. |
| | `has_indirect_passenger(entity, other)` | Vanilla `hasIndirectPassenger`. |
| | `data(entity, key, out)`, `set_data(entity, key, nbt)` | A mod compound saved with the entity under `key` (Pumpkin's entity `data`; NeoForge `getPersistentData()`), Java named-root NBT up to 64 KiB, empty removes. Keys use the `ResourceLocation` charset, at most 64 bytes. Appended: require `EntitiesExtension::Data`. |
| | `may_build(entity)` | Vanilla `Player.mayBuild()`: false for other entities and for Adventure and Spectator players. Appended: require `EntitiesExtension::MayBuild`. |
| | `spawn_carrier(world, kind, position, key, nbt)` | A saved, non-living carrier with mod data `nbt` under `key` set before it's added: `CARRIER_MARKER` is a vanilla marker (never sent to clients, for Super Glue selections and similar), `CARRIER_SEAT` an empty, invisible `item_display` whose passenger seat is its own position (clients draw the rider sitting there). Returns the id or -1. Appended: require `EntitiesExtension::Carriers` (this row and the next five, plus the `ENTITY_NO_PHYSICS` and `ENTITY_PASSENGER` flags). |
| | `discard(entity)` | Vanilla `Entity.discard()`: removed without drops or a save, riders dismounted. False for players and removed entities. |
| | `start_riding(entity, vehicle, force)` | Vanilla `Entity.startRiding(vehicle, force)`: refuses its current vehicle, itself and ride cycles; without `force`, a sneaking rider or a vehicle with passengers; dismounts from an earlier vehicle; fires the mount events and sends the passenger list. True when it now rides `vehicle`. Pumpkin has no boarding cooldown, so that part of `canRide` is absent. |
| | `stop_riding(entity)` | Vanilla `Entity.stopRiding()`, living riders placed at their dismount position. False when not riding. |
| | `vehicle(entity)` | The ridden entity's id, 0 for none. |
| | `entities_matching(world, min, max, except, exclude, skip, out, capacity)` | `entities_in` with entities carrying any `exclude` flag bit filtered out before counting, and the first `skip` matches left out, so a caller pages through any number of results. Returns the matches after `skip`. |
| `moddedmc:interaction` v1 | `on_use_on_block(reg, callback)` | Stage one callback per mod, called like vanilla `Item.useOn` for any of the mod's content items, block items included. It gets the world, the player (an entity reference), the held stack and the context: item id, clicked position and face, hit point, sneaking and creative. `PASS` lets the server place the item's block as usual. |
| `moddedmc:interaction` v1 extension | `on_vanilla_use_on_block(reg, callback)` | Stage one callback per mod for vanilla items at the same item-use phase, before the item's built-in behavior. Returning `PASS` leaves the stack and world unchanged and continues to the next callback, then vanilla. The first handled result ends item dispatch. Require `InteractionExtension::VanillaItemUse` before registration. |
| | `place_held_block(player, clicked, face)` | Place the current hand's block during a content block use through the normal player placement pipeline. The caller spends the item for each successful placement. Require `InteractionExtension::PlaceHeldBlock`. |
| | `on_item_use(reg, callback)` | Normal use for this mod's items. Borrows both hand stacks and passes the initial outline ray hit. Return 0 to continue default dispatch, -1 to handle the action, or a positive tick count on start to enter timed use. Finish and release receive the original hand only while it still holds the original item and components. A handled finish replaces default consumption. Require `InteractionExtension::ItemUse`. |
| | `item_use_animation(reg, item, ticks)` | Add held-use prediction to the stock Java view of a registered standalone item. The client can release early. The server still uses the callback's timer and never consumes the rendered carrier. Require `InteractionExtension::ItemUse`. |
| | `UseBlock::action_bar(text)`, `ItemUse::action_bar(text)` | Send an action bar message to the player using a block or item. The SDK checks the appended field and skips the message on older hosts. |
| | `on_melee_damage(reg, callback)` | Appended. Require `InteractionExtension::MeleeDamage`. The item's owning mod can modify a player's direct melee damage before target defenses. The callback receives the item ID, the target's registered mod mob ID (empty for vanilla), and the damage. Invalid return values preserve the original damage. |
| | `add_exhaustion(player, amount)` | Add food exhaustion through the vanilla player method, which skips invulnerable players and fires the exhaustion event. Require `InteractionExtension::Exhaustion`. |
| | `on_use_item(reg, callback)` | Immediate use of this mod?s content item from either hand, after the timed callback if it passes. Supplies the held stack and packet yaw and pitch. Require `InteractionExtension::ImmediateItemUse`. |
| | `on_player_placed(reg, callback)` | Observe a successful player placement of this mod?s content block. Require `InteractionExtension::PlayerPlaced`. |
| | `on_use_mob(reg, kind, callback)` | Appended. Require `InteractionExtension::MobUse`. Stage one right-click callback per registered mod mob type. It receives the world, player, mob, held stack, hand, sneaking and creative state. `PASS` lets the carrier handle the click. |
| | `stack_count`, `stack_shrink` | Read or shrink the held stack. |
| | `stack_data`, `set_stack_data` | Read or replace up to 1 KiB of mod bytes stored on the stack (`custom_data` key `moddedmc:data`). Empty removes them. |
| | `cooldown(player, item, ticks)` | Vanilla `ItemCooldowns.addCooldown`. |
| | `on_use_block(reg, callback)` | The block use extension, appended to the table (`InteractionExtension::BlockUse`). One callback per mod for its content blocks: vanilla `useItemOn` with the held stack (empty for an empty hand), then, when it returns `USE_TRY_EMPTY_HAND` for the main hand, `useWithoutItem` with an empty stack. It gets the position, state, face, hit point, sneaking, creative, the hand and which of the two it is. |
| | `on_player_will_destroy(reg, callback)` | Vanilla `Block.playerWillDestroy` for the mod's content blocks: called with the player, position, state and creative after the break is accepted, before drops and removal. Appended: require `InteractionExtension::PlayerWillDestroy`. |
| | `on_redstone(reg, signal, connects)` | Redstone output for the mod's content blocks. `connects(state, direction)` is vanilla `isSignalSource()` with NeoForge `canConnectRedstone(direction)` and decides where wire connects; `signal(world, pos, state, direction, direct)` is vanilla `getSignal`, or `getDirectSignal` when `direct`, called for every neighbour query as vanilla does, clamped to 15. Directions are vanilla's argument in order down, up, north, south, west, east. Block writes that change a state already notify neighbours, so wire and lamps update. Appended: require `InteractionExtension::Redstone`. |
| | `stack_read`, `stack_write` | The held stack as saved NBT, or replace it from saved NBT; empty clears it. |
| | `on_brewed_potion(reg, callback)` | Appended at byte offset 200 on 64-bit hosts. One observer per mod, called synchronously when a player takes a nonempty, non-glass-bottle stack from a brewing stand bottle slot, including shift-click and custom outputs. Callback receives borrowed world, player entity, item ID and saved stack NBT; it does not run when brewing completes or a hopper extracts. Require `InteractionExtension::BrewedPotion`. |
| | `compare_lodestone`, `stack_set_lodestone` | Setting a nonempty dimension assigns an untracked lodestone target while replacing stack mod data. An empty dimension **removes** the `minecraft:lodestone_tracker` component, restoring the stock compass spawn needle branch instead of retaining a present-but-empty tracker. The host resource pack uses `minecraft:has_component` to select the vanilla 26.3 spawn versus lodestone compass frame dispatcher; this is a stock-client model adaptation, not a graphical verification claim. |
| | `give(player, stack)` | Vanilla `Inventory.placeItemBackInInventory` for a saved stack: what doesn't fit drops at the player. Inside a use callback it merges into the used stack as vanilla's live held slot would, so read it again with `stack_read` afterwards. |
| `moddedmc:effects` v1 | `play_sound(world, position, sound, source, volume, pitch)` | Vanilla `Level.playSound(null, ...)` for players in range. Ids outside `minecraft:` play from the resource pack. `source` follows vanilla `SoundSource` order. |
| | `particles(world, particle, position, spread, speed, count)` | Vanilla `ServerLevel.sendParticles` for a particle type without options. |
| | `play_entity_sound(world, entity, sound, source, volume, pitch)` | Send a sound bound to an entity to its Java trackers and to the entity itself if it is a Java player. The client follows the entity and stops playback when it is removed. Returns false if the emitter is missing or untracked. Appended: check `Effects::require_entity_sound()`. |
| | `particles_with_options(world, particle, position, spread, speed, count, options, viewer)` | Appended at byte offset 32 on 64-bit hosts. Typed `ParticleOptions` carries an item registry id for `minecraft:item`, a runtime block state for `minecraft:block` or `minecraft:falling_dust`, packed RGB and scale (clamped to 0.01–4) for `minecraft:dust`, or no data for simple particles. Viewer -1 broadcasts in the world; otherwise only that loaded player entity receives it. False for invalid options or viewer. SDK: `Effects::require_particle_options()` and `ParticleOption`; older hosts return false without reading beyond their table. |
| | `on_effect(reg, kind, callback)` | Append at byte offset 40 on 64-bit hosts. Stage one callback per registered mod mob kind. Before a pending effect is applied to a loaded mob, call `callback(world, mob, effect_id, duration, amplifier) -> bool` with its Minecraft registry id, duration in ticks and zero-based amplifier. False refuses the effect, including refreshes. Vanilla immunity still applies; the SDK treats a panic as allow. SDK: `Effects::require_effect_veto()` and `Effects::on_effect()` (one shared handler per mod). |
| `moddedmc:resources` v1 | `list(directory, out)` | Vanilla `ResourceManager.listResources`: every file under `data/<namespace>/<directory>/` across mod data, the world's datapacks and Pumpkin's bundled packs, as sorted `namespace:directory/...` ids. Directory packs only. Vanilla cooking and stonecutting recipes, which Pumpkin compiles in, are listed and read under `recipe` as their vanilla JSON when no pack overrides them. |
| | `read(id, out)` | The highest-priority pack's bytes for an id, at most 4 MiB. |
| | `generation()` | 0 before datapacks first load, then a new value after each load, `/reload` included. Calls read the disk, so cache parsed data and reparse when it changes. |
| `moddedmc:crafting` v1 | `match_recipe(world, width, height, grid, out)` | Match a row-major grid of saved stacks against the same recipes as the crafting table, including loaded datapacks. Empty slices are empty slots. Each side is 1 to 9. The sink receives the product as saved NBT at key 0, the recipe id when known at key 1, and the matching engine serializer id at key 2. The query leaves the grid untouched. |
| | `remainder(stack, out)` | The ingredient's default crafting remainder as saved NBT, such as the bucket from a milk bucket. Returns false when absent. |
| | `enumerate_recipes(world, width, height, grid, limit, out)` | Appended to the original crafting table. Enumerate up to 64 actual matching vanilla and datapack recipes in normal engine order, without changing the default match. The sink emits triples at keys `index * 3` (saved result-stack NBT), `index * 3 + 1` (recipe ID when known), and `index * 3 + 2` (serializer ID). Pumpkin's generated vanilla crafting definitions contain no source IDs, so they use synthetic `moddedmc:generated_crafting/v2/<128-bit-definition-fingerprint>` identifiers, stable across table reordering rather than tied to a generated ordinal; these are **not** Mojang resource-location IDs. Previously stored `moddedmc:generated_crafting/<table-index>` choices are deliberately invalidated, not silently mapped to a different recipe. Real datapack recipe IDs are preserved. Return bits 0–13 contain emitted count; bit 15 (`0x8000`) means more matching recipes exist. Invalid grids/worlds return 0. Use `Crafting::enumerate_recipes` and check `Crafting::supports_player_selection()` before relying on the appended table. |
| | `current_choices(player, limit, out)` | Enumerates the player's current 3×3 crafting table or 2×2 inventory grid, using the same sink layout and limits. Bit 14 (`0x4000`) means a supported crafting screen was found, even when it has no matching recipes. No menu bit means the request cannot access a supported screen. The function does not choose or consume anything. |
| | `select_recipe(player, index)` | Pins the zero-based match index (0–63) to that player's active crafting result slot. An out-of-range, stale or nonmatching choice fails without granting items. The selected result, ingredient use and remainders remain coupled through the engine's ordinary take/shift-click code, and a real ingredient replacement or datapack catalog change revalidates the choice. Selection is menu-local and is not a persistent recipe preference. The host sends the updated slot while leaving the crafting screen open. This is a stock-client-compatible command surface, **not** proof of the original Polymorph graphical arrow UI or automation/crafter parity. |
| | `enable_result_cycle()` | Explicitly opt in to right-click result-slot cycling for ordinary stock Java clients on the current 2×2/3×3 crafting menu. Right-click with an empty cursor rotates through genuine matching results **without taking ingredients**, synchronizing the full menu revision to counter client-side pickup prediction. Left-click and shift-click still craft, and no Polymorph mod means unchanged vanilla behavior. This is a practical vanilla UI, not a claim of original mod overlay parity. |
| | `current_machine_choices(player, limit, out)` | Append-only capability using the exact same bounded sink/flag format as `current_choices`. Requires a non-spectator player with a genuine placed vanilla Crafter menu open; enumerates recipes against a frozen snapshot of that machine's current nine slots without consumption. Older hosts omit this appended field and the SDK returns `None`. |
| | `select_machine_recipe(player, index)` | Append-only machine-local selection for the genuine placed Crafter currently opened by the player. The host checks live block-entity incarnation, exact open menu, ingredient snapshot and recipe catalog revision, persists the selected definition identity on the machine, and updates the stock Crafter's read-only result preview. No synthetic item grants. Stale or invalid choices fail. This is not a durable ingredient/output transaction or visual parity proof; ordinary vanilla Crafter behavior remains the default without Polymorph. |
| `moddedmc:tags` v1 | `values(registry, tag, out)` | A tag's entry ids by string, so content blocks and items count: Pumpkin's generated vanilla entries plus every loaded pack's, nested tags flattened, sorted. A pack's `replace` drops lower packs' and the generated vanilla entries, as in vanilla. |
| | `contains(registry, tag, id)` | Whether the id is in the tag. Cache results per `moddedmc:resources` generation. |
| | `memberships(registry, id, out)` | The sorted tags that contain an entry, using the same pack merge as `values`. An unknown registry or an older host without the appended field returns `None` through the SDK. |
| `moddedmc:containers` v1 | `slots(world, position, face)` | NeoForge `Capabilities.ItemHandler.BLOCK` for the vanilla container at a position (what a hopper sees, the composter included), as `InvWrapper` or, for a sided container, `SidedInvWrapper` for that face. `NO_FACE` is the null side. -1 when there is none. |
| `moddedmc:container_conversion` v1 | `register(reg, callback)` | One synchronous callback per mod, registered before a world is loaded, proposing `Pass`, `Defer` or a destination block-state ID and complete named-root Java NBT. For replacement, the callback emits one named-root NBT value with the destination state ID as its sink key. Callbacks are consulted in mod load order, and the first non-pass decision stops the search. The host visits eligible loaded vanilla deferred-loot containers; proposals run without world/inventory locks and are not themselves writes. It validates NBT (at most 2 MiB), ownership of the registered destination state and unchanged source loot table/seed. A change commits only at the active-chunk migration boundary after loaded-neighborhood/post-processing checks and a source state/entity compare, and pending data plus the replacement entity are saved together with the state. `Defer` retains vanilla unopened loot and queues a later attempt. The initial Pumpkin conversion supports **only untouched single vanilla chests** with deferred `LootTable`, no nonempty `Items`, and no `Lock`/`lock` NBT; exposed inventories, double chests, barrels and other variants are not yet eligible. This is not a cross-file crash-atomicity guarantee. |
| | `stack`, `slot_limit` | `getStackInSlot` as saved NBT, `getSlotLimit` (brewing stand bottles 1). |
| | `insert(..., slot, stack, simulate, out)`, `insert_stacked(..., stack, simulate, out)` | `insertItem` and `ItemHandlerHelper.insertItemStacked`; the remainder goes to `out`. Includes NeoForge's one-bucket furnace fuel limit and `VanillaHopperItemHandler`'s cooldown when filling an empty hopper. |
| | `extract(..., slot, amount, simulate, out)` | `extractItem`, honouring `canTakeItemThroughFace` for a side. |
| | `furnace_heat(world, position, boost, min_lit, out)` | Read (zero boost and min_lit) or synchronously advance a loaded vanilla furnace, smoker or blast furnace. Mutations outside the world's serial content tick callback return false. Returns kind (1/2/3), cooking progress, total, remaining lit time and lit duration. With positive total, advance at most to total minus one, never reduce existing progress, raise remaining lit time and duration if below min_lit, update the lit block state and mark the entity dirty. `reached_end` is true only if this operation advances to total minus one. Never completes a recipe or loads chunks. Require the appended field with `Containers::require_furnace_heat`. |
| `moddedmc:item_input` v1 | `add(reg, block, receive)` | Register an item input for a content block staged by the same mod. Hoppers and droppers offer one item as saved stack NBT with the current block state and the receiving face. Return 1 to accept or 0 to keep the item in the sender. A simulation must leave the receiver unchanged. |
| `moddedmc:block_displays` v1 | `spawn(world, state, display)`, `update`, `set_state`, `remove` | Block displays that are never saved, for blocks that aren't in the world (a moving contraption). The display gives the position, the model translation, a left rotation quaternion, per-axis scale, the transformation blend in ticks (`interpolation_duration`, restarted on each update), the position interpolation (`teleport_duration`) and a brightness override. Content states use their baked model on stock clients. When a placed block hides its carrier under a static display, the host can assign a separate baked carrier for the moving display. |
| `moddedmc:item_stacks` v1 | `entity_stack`, `set_entity_stack` | Read an item entity's stack, or replace it; an empty stack discards the entity. Stacks are vanilla saved `ItemStack` NBT (`id`, `count`, `components`) as Java named-root binary, at most 64 KiB, so mods can keep them in their own block entity data. |
| | `fuel(stack, remainder)` | Regular furnace burn time from the stack's effective cooking fuel component. Returns zero for a non-fuel or invalid stack. The sink gets the crafting remainder as saved stack NBT when present. Require the appended entry with `StacksExtension::Fuel`. |
| | `has_maxed_enchantment(stack)` | Whether an applied enchantment meets or exceeds its defined maximum. The SDK returns false on older hosts without this appended entry. |
| | `spawn(world, position, motion, stack)` | Vanilla `new ItemEntity`, `setDeltaMovement`, `setDefaultPickUpDelay` and `addFreshEntity`, firing the plugin item spawn event like block drops. Returns the entity id. |
| | `info(stack, out)` | Count, vanilla `getMaxStackSize`, and whether the item places a block. |
| | `component(stack, component, out)` | Vanilla `ItemStack.get(component)`: the patch over the item defaults, as `{value: <component>}` named-root NBT, an empty root for unit components or ones Pumpkin doesn't encode yet. False when absent. Appended: require `StacksExtension::Component`. |
| `moddedmc:displays` v1 | `spawn_item(world, stack, display)` | An item display that is never saved, for things that aren't entities (items on a belt). The display gives the position, a left rotation quaternion, a uniform scale, the `ItemDisplayContext` and the client's position interpolation in ticks. Returns its id. |
| | `update`, `set_item`, `remove` | Move or restyle it, change its stack, or take it down. `update` returns false once the display is gone (its chunk unloaded); spawn a new one then. |
| `moddedmc:mobs` v1 | `add_mob(reg, definition)` | Stage a mob type: its id, the vanilla living type that carries it (`minecraft:ravager`), and base attribute values set when it spawns (then saved like any attribute). Stock clients see the carrier. |
| | `spawn(world, kind, position)` | Spawn a registered mob type, or a vanilla mob by id. Returns its entity id, or -1. A registered mob starts at full health for its `max_health` and drops the loot table `<namespace>:entities/<path>` of its id when it dies, not its carrier's; ship it in the mod's `data/`. |
| | `kind(entity)` | The registered type of a mod mob, or empty. It is kept in the entity's custom data (`data.moddedmc.mob`), so it survives restarts; a mob whose type is no longer registered reads as empty. |
| | `state(entity, out)`, `set_state(entity, bytes)` | The mod's own bytes on a mod mob, at most 64 KiB, saved with it (`data.moddedmc.state`). False for anything that isn't a mod mob. |
| | `on_death(reg, kind, callback)` | Appended. Require `MobsExtension::Taming` for this and the remaining mob calls. Run one callback per registered mob type before its death loot drops. |
| | `uuid(entity, out)`, `by_uuid(world, uuid)` | Read a 16-byte entity UUID or find a loaded entity by UUID in this world. |
| | `health`, `heal`, `passengers`, `tame_effect` | Read current and maximum health, heal a living entity, list direct passenger IDs in seat order, or broadcast the vanilla taming status. |
| | `set_no_gravity`, `set_on_fire`, `reset_hurt_cooldown` | Set the gravity flag, ignite an entity for a duration (zero extinguishes it), or reset a living entity's damage cooldown. |
| | `player_input`, `game_mode`, `message` | Read a player's latest movement key bits or game mode, or send plain text to chat or the action bar. |
| | `make_persistent` | Prevent a mob from naturally despawning. Pumpkin saves the flag with the mob. |
| | `add_spawn(reg, definition, check)` | Appended. Require `MobsExtension::NaturalSpawns`. Add a mob type the mod registered to the biome spawn pools (vanilla `MobSpawnSettings`) for a biome id or `#tag`: a `MobCategory` name, weight, group size and `spawn_location`. The natural spawner and chunk generation pick from vanilla and mod entries by weight; the mod mob uses its carrier's size, mob cap and despawn rules, optionally its carrier's spawn rules too, then `check` (vanilla `SpawnPlacements`) with the position, biome, the summed weight of the pool's vanilla entries and whether it is chunk generation. |
| | `award_advancement(player, advancement, criterion)` | Appended. Require `MobsExtension::Advancements`. Grant one criterion of an advancement, vanilla or from a mod's datapack, to a player, as vanilla `PlayerAdvancements.award` does; the advancement completes when its requirements are met. True when this call granted it. |
| | `kill_credit(EntityRef) -> i32` | Appended at byte 184. Vanilla living entity kill credit, including the last attacker within the native timeout; -1 if absent. |
| | `player_sleeping(EntityRef) -> i32` | Appended at byte 192. Vanilla player sleeping state: 1 asleep, 0 awake, -1 for a non-player. |
| | `award_experience(WorldRef, Vec3, u32) -> bool` | Appended at byte 200. Spawn experience orbs at a finite position with a positive total value. |
| | `award_stat(EntityRef, Str, Str, i32) -> bool` | Appended at byte 208. Increment a player's vanilla statistic by a positive amount, using the category and registry key (e.g. `minecraft:custom`, `minecraft:animals_bred`). |
| | `boolean_gamerule(WorldRef, Str) -> i32` | Appended at byte 216. Read a named boolean rule: 0 false, 1 true, -1 unknown or non-boolean. Uses the host's current keys (e.g. `minecraft:mob_drops`, formerly `doMobLoot`). All five appended SDK calls return `None` if unavailable; mutating calls return `Some(false)` on rejection. |
| | `last_hurt_mob(EntityRef, *mut i64) -> i32` | Appended at byte 224. Last living target damaged by the entity (cleared when the target dies or leaves the world); writes the overworld game time of the hit, or returns -1 without writing if absent or not living. |
| | `last_hurt_by_mob(EntityRef, *mut i64) -> i32` | Appended at byte 232. Remembered living attacker (subject to the vanilla attacker timeout); writes the overworld game time of the hit, or returns -1 without writing if absent or not living. Require `MobsExtension::CombatHistory` for both calls; SDK returns `Option<(i32, i64)>`. |
| | `on_merchant_close(reg, kind, callback)` | Appended at byte 240. Require `MobsExtension::MerchantClose`. Stage one callback per registered mob kind: `fn(world, player, merchant)` when the player's screen opened by `open_trades` closes and the merchant is still loaded. All handles are borrowed for the call. |
| | `pet_owner(entity, out_uuid)` | Byte 248. Native mob owner UUID (16 bytes), if present. |
| | `team(entity, sink)` | Byte 256. Native scoreboard team name, copied to the sink if present. |
| | `allied(entity, other)` | Byte 264. Native entity alliance query. |
| | `set_seat_offset(entity, index, offset)` | Byte 272. Override a registered mod mob's passenger attachment, relative to its yaw. Indices 0–15; finite offsets within 64 blocks per axis. |
| | `set_dismount_offsets(entity, offsets, count)` | Byte 280. Copy up to 512 ordered local candidate offsets. At dismount, rotate by current yaw and test native safe floor/clearance; fall back to native dismount search if none qualify. An empty slice clears the candidates. Require `MobsExtension::PetsSeats` for all five hooks. |
| | `open_trades(mob, player, offers)` | Appended. Require `MobsExtension::Trading`. Open a villager carrier's vanilla trading screen for a player. Non-empty `offers`, vanilla's saved `Offers` compound as NBT, first replace the carrier's offers; the carrier then counts uses and saves them. False when the mob isn't a villager. |
| `moddedmc:mob_ai` v1 | `add_goal(reg, definition)` | Stage a goal for a registered mod mob type: a key, a vanilla priority, `goal_control` bits (`MOVE`, `LOOK`, `JUMP`, `TARGET`) and a callback. Every mob of the type gets it as it joins a world, spawned or loaded. The callback is asked each tick with a `goal_phase` (`CAN_START`, `CONTINUE`, `START`, `STOP`, `TICK`), the world and the mob; only the answer to `CAN_START` and `CONTINUE` counts. Goals with `TARGET` run in the target selector. |
| | `replace_goals(reg, kind)` | Mobs of the type drop their carrier's own goals and targets before getting the mod's. |
| | `walk_to(mob, target, speed)`, `stop(mob)`, `navigating(mob)` | Path to a position at a multiple of the movement speed, drop the path, or ask whether one is being followed. |
| | `look_at(mob, position)` | Turn the head toward a position this tick. |
| | `set_target(world, mob, id)`, `target(mob)` | Set or clear (-1) the attack target by entity id, or read it (-1 when none). |
| | `on_hurt(reg, kind, callback)` | Appended; require `MobAiExtension::Combat`. Hear every hit a mob of the type is about to take, with the damage type id (`minecraft:fall`) and the amount before armour, as vanilla `hurt` starts. Returning false cancels the hit. One callback per type. |
| | `last_hurt_by(mob, timestamp)` | Appended. Vanilla `getLastHurtByMob` as an entity id (-1 when none), and `getLastHurtByMobTimestamp`. |
| | `attack(world, mob, target, amount)` | Appended. The mob hits a loaded entity with vanilla's mob attack damage source. True when it took the hit. |
| | `damage_source(causing, direct)` | Appended; require `MobAiExtension::DamageSource`. In a mob hurt or worn hurt callback, write the causing entity (e.g. shooter) and direct entity (e.g. arrow) ids to the output slots, -1 for either absent. Returns false outside these callbacks. |
| | `on_hurt_final(reg, kind, callback)` | Appended; require `MobAiExtension::HurtFinal`. Receives the post-armor and magic amount, whether it would exhaust absorption and health, and the current absorption. Returns an adjusted nonnegative amount before absorption and health loss. Non-finite results leave damage unchanged. The damage source is available for this callback too. One callback per type. |

Require the tables from `Mod::start` and register definitions in `Mod::register`. Requesting `moddedmc:content` or `moddedmc:items` during startup moves that mod's registration before world load. Legacy mods continue to register on the first tick. State numbers are runtime handles. Store names and properties in mod-owned data. Mods with a placement hook can choose one of the placing block's states from the clicked face, look direction, sneak state and loaded neighbors.

The Rust SDK exposes them through `moddedmc_api::content`, `moddedmc_api::items`, `moddedmc_api::stacks`, `moddedmc_api::interaction`, `moddedmc_api::effects`, `moddedmc_api::mobs` and `moddedmc_api::mob_ai`. See `mods/create` for registration, placement, loading saved NBT and processing batches, and `mods/saints-dragons` for mob types.

## Copper weathering (`moddedmc:weathering_copper` v1)

`register(reg, ids)` accepts eight distinct IDs of blocks already staged by the same mod:
unaffected, exposed, weathered and oxidized, followed by their waxed equivalents. All
eight must have the same state selectors, use the default content entity, and have no
mod-defined random tick. Unwaxed blocks participate in vanilla oxidation and
lightning cleaning with vanilla copper. Honeycomb and axes use the vanilla waxing,
scraping and unwaxing paths, preserving matching state properties. The family is
rebuilt from the registration before each world opens.

## Item defaults (`moddedmc:item_defaults` v1)

This pre-world interface starts with its byte size and exposes
`add_item(Reg, *const ContentItemDefinition, Str) -> i32`. The last argument
is named-root binary `ItemStack` NBT for one item of the declared vanilla
carrier. Explicit persistent components in the NBT override the carrier's
defaults. The host rejects component removals. Explicit per-stack changes still
apply, and the Java wire stack includes the default overrides so the client
receives the same tool and durability values. Use `items::require_defaults()`
at startup to require the interface.

For a custom record, set `minecraft:jukebox_playable` to an inline song
definition and ship the same definition in `data/<namespace>/jukebox_song/`.
The song specifies `sound_event` (a compound containing `sound_id` and an optional
`range`), `description`, `length_in_seconds`, and `comparator_output`. The
matching datapack entry supplies the synced song ID for position-specific
jukebox playback and ejection. Ship the referenced sound in the resource pack.

The appended `moddedmc:block_world` v1 field `is_thundering(world)` reads the
world's thunder state. Require `WorldExtension::Weather` before using it.
`dimension_id(world)` returns the loaded world's static dimension identifier.
Require `WorldExtension::DimensionId` before using it.

The appended `moddedmc:block_world` v1 fields `in_biome(world, pos, biomes)`,
`surface_height(world, x, z, heightmap)` and `can_see_sky(world, pos)` answer
spawn placement questions: whether the loaded biome is a biome id or `#tag`
(1, 0, or -1 when unloaded), vanilla `Level.getHeight` for a `heightmap` type
(`i32::MIN` when unloaded or unknown), and vanilla `Level.canSeeSky`. Require
`WorldExtension::Surface` before using them.

## Mob storage (`moddedmc:mob_storage` v1)

`save(entity, out)` writes a live registered mob's named-root NBT with its
UUID and mod data, excluding passengers. It never removes the entity.
`restore(world, kind, nbt, at)` restores that registered carrier and UUID at an
unoccupied, loaded position and clears movement and fall distance. A live
duplicate UUID, invalid data, or rejected spawn returns -1. The SDK exposes
the calls through `mobs::require_storage()`. The saved data is limited to 64 KiB.

`moddedmc:interaction` v1 appends `on_use_item(reg, callback)`
(`InteractionExtension::ItemUse`). One callback per mod receives the held
content item, world and player, and the look angles for the action. It edits
the borrowed hand stack through the existing stack calls. Success or consume
writes the hand back. Pass and fail leave that slot alone.

`moddedmc:entities.damage` (`EntitiesExtension::Damage`) adds `Entity::hurt`.
It resolves a registered damage type, including types defined in mod datapacks,
and applies damage without an attacker through the normal entity damage path.
The return value reports whether the hit took effect.

`moddedmc:entities.player` (`EntitiesExtension::Player`) exposes a borrowed
player handle for a player entity, so item and entity callbacks can use the
SDK's dialog and other player methods. The handle shares the entity callback's
lifetime. A player callback can also borrow the player's current world and entity
through `Player::with_entity_context`.

`moddedmc:interaction.player_placed` (`InteractionExtension::PlayerPlaced`)
notifies a mod after a player successfully places one of its registered blocks.
The callback borrows the world and player and receives the placed state and
position. The block entity has been initialized when the callback runs.
Commands and world generation use their own placement paths.

## Arrow ammunition (`moddedmc:arrow_ammo` v1)

`bind(reg, item, velocity_multiplier, water_inertia, after_hurt)` makes an item
staged by this mod with the `minecraft:arrow` carrier usable in bows and
crossbows. The velocity multiplier is finite and positive. Water inertia is
between 0 and 1. A successful hit against a living entity invokes
`after_hurt(item, target)` synchronously after normal arrow damage and effects.
The item id and target reference live for the call. The SDK requires
`moddedmc:entities` for a usable target reference and uses one callback per mod
for its bound arrows. The projectile saves its ammunition stack and pickup
rules with the chunk.

## Worn and held equipment (`moddedmc:equipment` v1)

The hook calls stage a hook for an item this mod registered. Each item takes at
most one hook of each kind. The SDK passes one handler per mod for each kind,
and the handler receives the item id. Require the table with
`equipment::require()` in `Mod::start`.

- `on_worn_hurt(reg, item, callback)`: before a player wearing `item` in a head,
  chest, legs or feet slot takes damage. The callback borrows the world and
  player, the four armor item ids in head, chest, legs, feet order
  (`minecraft:air` when a slot is empty), the damage type id, the amount and the
  player's fall distance. It returns the damage to take. A negative value
  cancels the hit. A player wearing several hooked items gets each distinct
  item's hook in that slot order, each with the amount the last one returned.
- `on_weapon_hit(reg, item, callback)`: during a direct melee hit by a player
  holding `item` in the main hand, after the `moddedmc:interaction` melee
  modifier and before the target's defenses. The callback borrows the world,
  attacker and target, the damage type id and the amount, and returns the
  damage to deal. A negative or non-finite result keeps the original amount.
- `worn_armor(player, out)`: read the player's four namespaced armor item IDs
  in head, chest, legs, feet order, with `minecraft:air` for empty slots. The
  sink receives one newline-separated UTF-8 value at key 0; false for a
  non-player or invalid handle. This is an appended optional field; the SDK
  returns `None` on older hosts that do not provide it.
- `on_successful_weapon_hit(reg, item, callback)` (offset 32 on 64-bit): once
  after a successful direct player melee hit on a living target, including a
  lethal hit, with the borrowed world, held item ID, attacker, target and
  actual net health plus absorption lost. It is not called for rejected hits,
  sweep targets, projectiles or damage invoked by another callback. Require
  `Equipment::require_successful_weapon_hit` at startup.
- `EntitiesV1::is_attackable(entity)` (offset 408 on 64-bit): whether the
  entity is living, alive and can be hit (excludes spectators and marker armor
  stands). Damage immunity is checked by
  the damage operation itself. Require `EntitiesExtension::Attackable`.

## Fish buckets (`moddedmc:fish_buckets` v1)

`bind(reg, item, mob)` associates an item carried by `minecraft:cod_bucket` with a custom mob carried by cod, salmon, tropical fish, or pufferfish. Both definitions must already be staged by the same mod. Each item and mob can participate in one binding. Pumpkin handles water-bucket pickup, release, and the carrier's bucket data, including health, name and persistent flags. The item and released fish keep their registered mod identities.

## Structure mobs (`moddedmc:structure_mobs` v1)

Structure templates can name any registered mob by its namespaced id. The host creates
the mob with its registered carrier and attributes at the template's transformed
position. The host applies the template's rotation, health and persistence flag.

During registration, `on_load(reg, kind, callback)` attaches an initializer to a mob
staged by the same mod. Before the mob joins the world, the initializer receives its
borrowed handle and the named-root template entity NBT, up to 64 KiB. Mods can copy
their own saved properties onto the entity during the call. The normal mob spawn,
binder restore and chunk reload paths already have their own state and skip this hook.

## Mob loot (`moddedmc:mob_loot` v1)

`MobLoot::suppress` assigns the empty death loot table to a registered mob.
The override is saved with the mob. Equipment drops and experience have their
own rules.

## Direct mob health (`moddedmc:mob_health` v1)

The standalone table starts with its byte size and exposes
`set_health(EntityRef, f32) -> bool`. It accepts finite positive health up to
the maximum of a living registered mod mob. The operation updates tracked
health directly and skips attack or healing events. Use
`mobs::require_health()` at startup and `MobHealth::set` in a callback.

The appended `moddedmc:mob_health` v1 field `set_attribute_base(entity, attribute, value)`
updates a vanilla base attribute on a registered mod mob. Require it with
`MobHealth::require_attributes()` before using it.

## Save-global data (`moddedmc:saved_data` v1)

This table exposes a save-global NBT snapshot to drop-in mods without tying data to a
loaded chunk, block entity or process-local world pointer. It is available by exact
`HostApi::get_interface("moddedmc:saved_data", 1)`; the table begins with its byte `size`
and contains:

- `register_key(Reg, Str name, *mut u64 out) -> i32`: stage a mod-local name (1–128
  lowercase ASCII bytes: `a-z0-9_.-/`, no empty, `.` or `..` path segments). A successful
  registration returns a nonzero process-local handle; the handle is activated only when
  the entire mod registration succeeds. Duplicate names within a registration fail.
- `read(WorldRef, u64 handle, Sink) -> bool`: return the complete value under sink key 0,
  or false if it has never been written or the handle/world is invalid. An empty stored
  payload is distinct from a missing key.
- `write(WorldRef, u64 handle, Str bytes) -> bool`: atomically replace the in-memory value;
  return false for invalid input or a payload larger than `MAX_SAVED_DATA_BYTES` (8 MiB).
  No borrowed pointer is retained. Data is scoped by the mod id associated with its
  registration handle; another mod cannot accidentally register the same persistent key.

The payload is opaque bytes. Mods can use named-root binary NBT, JSON or their own stable
encoding. The host stores owner-qualified keys in a save-global snapshot at
`<save>/moddedmc/saved_data.nbt`, separate from per-dimension world data. It loads once per save and flushes dirty data on world save and shutdown via an atomic temporary file rename. Corrupt or unreadable files fail closed without silent replacement. A crash before world save can lose recent writes; `write` does not flush immediately. Each save is bounded to 64 MiB and 4096 keys and shared by all its dimensions. The host never loads chunks; `WorldRef` is borrowed for the current callback only.

Mods require this interface in `Mod::start`, reserve their handles in `Mod::register`,
and read from the first appropriate world callback after worlds load (e.g. the registered
Create content-tick callback). Do not read a world in `Mod::start`; do not persist
handles across restarts. Use SDK `saved_data::{require, SavedData, Key}`. A server lacking
the named table fails the mod's `start` requirement without changing ABI major 1.

## Player death drops (`moddedmc:player_death` v1)

`on_drops(reg, callback)` stages one callback per mod. Call it through
`player_death::require()` in `Mod::start` and
`PlayerDeaths::on_drops` in `Mod::register`. The callback borrows the player,
world, UUID, death position and a batch of saved ItemStack NBT. Slots
0–8 are the hotbar, 9–35 are storage, 36 is feet, 37 is legs, 38 is chest,
39 is head and 40 is the offhand. Use the world to query the death dimension.

The host invokes the callback after removing the inventory stacks and
releasing the inventory locks. Empty batches, retained inventories and
items destroyed by Curse of Vanishing do not enter the callback. Call
`Death::claim(key, record)` to save a record of the entire batch under
one of the mod's saved-data keys. The host replaces the save file before
committing the claim. A failed write restores the previous value and leaves
the batch available. The first successful claim suppresses world drops and
stops dispatch, even if the callback fails after it committed. If no mod
claims the batch, the engine drops each stack normally. Callbacks for
different players can overlap.

The claim covers one live death event and its committed data can be read
after a normal restart. Pumpkin saves player inventory and mod-owned data
in separate files, so cross-file crash recovery requires a joint journal.

`tests/scenarios/abi-player-death.txt` exercises accepted and declined
claims, a failed write, keepInventory and saved data after a restart.

## Inventory menus (`moddedmc:inventory_menu` v1)

Use `inventory_menu::require()` at startup and `Menus::register` to register
one change callback. `Menus::open` opens a stock chest screen with one to six
rows. Each slot has independent take and insert permissions and carries saved
ItemStack NBT. Views with the same world, menu kind and inventory ID share one
slot array while any view is open. The host calls the initializer on the first
open to read the latest saved slots. Slot permissions apply to each view.

The host stages vanilla clicks against the displayed slots, player inventory
and cursor. If a click changes mod-owned slots, the callback receives the
complete proposed slot array and the acting player's UUID. Call
`Change::commit(key, record)` with the mod's replacement saved-data value.
Only a successful write applies the click. A declined or failed write
preserves the menu, player inventory, cursor and pending drops. Keep the
callback focused on validation and persistence, as the host holds the
inventory locks until it returns.

If multiple inventories share one saved-data record, the mod must serialize
read-modify-write operations on that record, including writes from other hook
types. Acquire the mod's record lock inside the change callback and initializer.
Release it before calling any operation that opens or mutates a menu.

`Menus::restore_matching` works on the active view. Pass unique source slots
and unique destination slots in the order to restore. Player slots use the
same indices as the death-drop interface. Once all matching stacks are in
place, the host tries to pack displaced stacks and stacks marked
`ANY_PLAYER_SLOT` into free inventory space. The callback receives any
remainder in `Change::overflow()`. Include those stacks in the replacement
record to keep them available to the player.

Mods with clickable controls require `inventory_menu::require_actions()` at startup.
Use `Actions::menus()` to register the inventory kind, then `Actions::on_action`
to register the control callback. `Actions::open` takes the world that supplied
the items, unique control slots, and their mod-chosen tags. Each control slot
must forbid taking and inserting.
The initializer supplies an ordinary item stack as the control's display.
All live views of one inventory must declare the same control slot positions.

A left click with an empty cursor calls the action handler with the player, world,
inventory ID, and tag after releasing the screen lock. The handler can restore
matching slots or open another view. Other click modes leave the control item
in place. Existing mods can continue using the original v1 table prefix.

For a position- or identity-bound inventory, use
`inventory_menu::require_validity()`. Register a change callback and a
read-only validity callback with `Validated::register`. Open with
`ValiditySlots { world, access, context }`; the host copies at most 4096
bytes of immutable per-view context and checks that the player remains in
the opening world. The callback receives the live player, world, inventory
ID and copied context before opening, during screen ticks and before
clicks or matching-slot restoration. Returning false rejects interaction
and closes the invalid screen through the existing player tick path.
Keep the check inexpensive: validate the backing block/entity identity
and reach directly, without loading chunks, reading whole archives, opening
menus, mutating inventory or acquiring locks held by menu callbacks.
Change callbacks must still validate backing state at commit time.

For externally mutable inventories, require the appended v1 managed extension
with `inventory_menu::require_managed()`. On 64-bit hosts the table extends
the 56-byte validity prefix with `register_managed` (56),
`open_managed` (64), and `mutate` (72), for a total of 80 bytes.
Older hosts return interface unavailable.

A mod may register one legacy menu kind and one managed menu kind together.
Their callbacks are independent. The v1 action registration attaches to the
most recently registered kind; register each action immediately after its kind.

`Managed::register` registers the change, deferred action, validity,
insertion predicate and quick-move routing callbacks. `Managed::open`
combines controls and validity; it copies the immutable context. Unlike
legacy views, all managed viewers share the same access permissions.
The initializer reads the backing record only when creating the first view.
Return false if the access permissions supplied at open no longer match
that record. Control positions must be identical in all live views.

The insertion predicate receives context, slot index and candidate stack NBT.
The routing callback receives context and stack NBT and returns a bit mask
of eligible destination menu slots (bits 0–53), with bit 63 allowing
fallback to the other player inventory section. Taking, insertion and
quick moves still obey shared permissions. These callbacks are read-only.

`ManagedChange::commit_external` invokes a synchronous backing-record
write under the inventory transaction lock. Return true only if the write
committed, and false only if the backing record remains unchanged.
Use an identity-bound compare-and-write for block entities. The saved-data
commit remains available through `ManagedChange::change`.
`Managed::mutate` accepts a world, kind, inventory ID, complete replacement
slot NBT and access arrays, and a synchronous persistence closure. It
serializes with clicks and updates shared state only after successful
persistence, including when there are no viewers. Controls must remain
nontransferable. Viewers receive the replacement in their normal screen
updates; a click against stale projected slots/access is rejected and
resynchronized. Call mutate from deferred actions or world hooks, never
from a change, predicate, initializer, or validity callback. Release mod
record locks before calling menu operations; acquire them inside the
persistence closure. Managed views do not support legacy restore_matching.

These transactions cover live interactions and clean restarts. Cross-file
crash recovery between the player save and mod data requires a joint journal.

`tests/scenarios/abi-inventory-menu.txt` exercises taking and quick moves,
two viewers, rejected changes, and equipment restoration with overflow.
`tests/scenarios/abi-inventory-actions.txt` exercises matching-slot restoration
and opening another view from controls using vanilla menu packets.

## Persistent nonliving actors (`moddedmc:native_entities` v1)

Require the interface in `Mod::start` and use `Actors::register` to register
one actor kind per native mod. `Actors::spawn` creates a stock
`minecraft:interaction` carrier in a loaded chunk with the given width,
height, position, UUID and opaque state. Use a stable nonzero UUID to identify
an actor across saves. Spawn rejects an already loaded UUID.

Actor events borrow the world and actor. The host delivers `Loaded` when a
carrier joins the world, `Tick` after the world's parallel entity workers
have joined, and
`Used` with the acting player on a right click. Return true from `Used` to
consume the use. `Unloaded` follows a chunk unload and leaves the saved actor
available. `Removed` follows permanent removal. The state is still readable
in both callbacks. The other phases ignore the callback's return value.

Use `Actor::data` and `Actor::set_data` to read and replace up to 64 KiB of
opaque bytes. The carrier saves this data with the entity. It stays nonliving
and the host applies no gravity. Set `Spawn::physics` to enable collision for
movement that the mod applies through `actor.entity`.

`Actors::require_visual` checks the visual extension. `Actor::show_sleeping`
attaches a stock mannequin or skeleton to an actor.
The model follows the actor with the supplied offset, and Java right-clicks on
the model are routed to the actor after the normal reach checks. The model
disappears with the actor and is recreated by the mod on `Loaded`.
`Actors::player_profile` captures the player's profile for the mannequin,
including available signed skin properties, visible skin layers, and the
main arm. The visual also accepts
saved item stacks for both hands and the four armor slots.

`tests/scenarios/abi-native-entity.txt` covers use, chunk unload and load,
clean restart, the two stock models, and permanent removal.

## Player actions (`moddedmc:player_actions` v1)

The optional Fabric companion advertises semantic key actions. Require this named
table in `Mod::start` to stage actions before worlds load. Its original C prefix is two
pointer-sized fields: `size: usize` (offset 0) and
`add(Reg, Str id, Str translation, Str default_key, u8 phases, PlayerActionCallback) -> i32`
(offset 8, prefix size 16 on 64-bit). The appended
`subscribe(Reg, Str id, u8 phases, PlayerActionCallback) -> i32` is at offset 16
(table size 24 on 64-bit). Registration copies the strings; duplicate custom ids are
rejected and definitions become visible only after successful mod registration.
The callback is `unsafe extern "C" fn(WorldRef, EntityRef, Str id, u8 phase)`.
All handles and the id are borrowed for the call. The phase mask selects
pressed (1), held (2), and released (4); the callback receives one phase at a time.
The Rust SDK exposes `player_actions::require` and `PlayerActions::add`.
Use a single Rust callback per mod and dispatch by action id.

`PlayerActions::require_mounted` checks the appended field. Subscribe with
`PlayerActions::subscribe` and the SDK constants for `moddedmc:mounted_attack`,
`moddedmc:mounted_use` (pressed, held, released), and
`moddedmc:double_tap/forward`, `moddedmc:double_tap/back`,
`moddedmc:double_tap/left`, `moddedmc:double_tap/right` (pressed only).
Each registered mod may subscribe once to each built-in; callbacks are dispatched
to every subscriber of the requested phase. Direction is encoded in the id to
preserve the existing callback signature. Mounted inputs use the player's
current vanilla key mappings and do not create additional controls. Attack and
use are intercepted while the host confirms that the player rides a mod mob;
movement keys still steer normally. Double taps require two rising edges on
the same key less than 300 ms apart. Input state resets when the mount changes.

Use namespaced action ids, translation keys and Minecraft key names such as
`key.keyboard.r`, `key.keyboard.unknown` (unbound), or `ctrl+key.keyboard.4`.
The companion creates rebindable controls grouped by action namespace and stores
the chosen keys locally. Stock clients do not receive or send action packets.
An action namespace can ship `assets/<namespace>/lang/en_us.json` with its
translation keys and category label `key.category.moddedmc.actions.<namespace>`.
Mods must still validate gameplay conditions such as ownership, cooldown and riding.

Configuration capability bit 2 on `moddedmc:negotiate` enables the play channel
`moddedmc:player_actions`. Its protocol is version 1: client requests the
catalog with bytes [1,0] on join; server replies [1,1], a big-endian u16 count,
then each entry's id, translation and key as u8 byte lengths followed by UTF-8,
then its u8 phase mask. Client input is [1,2,phase,id-byte-length,id bytes].
Subscribed built-ins appear once in the catalog with key `moddedmc:mounted`,
translation equal to their id and the union of subscribed phases. A companion
queries a newly observed vehicle with [1,3,vehicle-id:i32 BE]. The host replies
[1,4,vehicle-id:i32 BE,confirmed:u8] (1 only if that vehicle is the player's
current registered mod mob). The companion requires a matching local vehicle
and confirmation before intercepting inputs; the host independently checks
current mounted status on every built-in action. The query shares the input
rate limit. The host can also send [1,5,duration:u32 BE] to request a bounded
fall-flying boost (0 cancels). The companion applies it only during eligible
flight; this packet is server-to-client only. Original v1 action packets and
catalog layout are unchanged.
The host caps the catalog at 128 actions, ids and translations at 128 bytes,
keys at 64 bytes, checks the action and phase, rejects dead players and
rate-limits input to 128 packets per second and repeats of the same phase
of an action to 40 ms (held: 150 ms). Held input is sent every four client ticks.

## Inter-mod services (`moddedmc:services` v1)

Mods can publish immutable, process-lifetime `#[repr(C)]` function tables to other native
`.mmc` mods. The service registry is a host interface discovered from `Mod::start` with
`moddedmc_api::services::require_host()`. Its C table begins with `size: usize` followed by
`publish(Reg, Str name, u32 version, *const c_void table) -> i32`. The service name uses
`<provider_mod_id>:<path>`; only a mod can register a service under its own namespace.
Versions are independent for each service. Duplicate (name, version) or invalid pointers
and names are rejected. No Rust-owned objects, string allocations or vtables cross the ABI.

The publisher uses `unsafe { services.publish(registry, "create:kinetics", 1, TABLE_PTR)? }`
from its `Mod::register`. The pointer must identify an immutable static table beginning
with its byte `size` and stay valid until the server exits. The host stages service
publication along with other mod definitions and makes it visible only after successful
registration. The SDK consumer uses `unsafe { moddedmc_api::services::require::<T>("create:kinetics", 1)? }`
inside `Mod::start`. `T` must match the provider's exact published C layout. The SDK
checks the table's byte size and fails initialization if it is unavailable; mods must
check the published table's `size` before reading any appended extension.

For a consumer to require a service during startup, its `.mmc` `mod.json` must declare
a `dependencies` array of provider mod IDs, e.g.

```json
{"id":"my_addon","version":"1.0.0","abi":1,"dependencies":["create"]}
```

The loader processes dependencies deterministically, registering providers before running
their consumers' `Mod::start`. Missing or failed providers and cycles skip the affected
mods with an explicit log message. A missing service version also fails `Mod::start`
without starting the consumer. Mods without dependencies retain existing startup order
and first-tick registration timing unless they request a pre-world content interface.
Providers needed by an addon register eagerly before worlds load.

Only explicit dependencies on native `.mmc`/other discovered mod IDs are supported;
this is not a Java classloader, Java `ServiceLoader` or a binary interface for Rust
trait objects. The table is synchronous and its callbacks must not unwind across C.
Tests: `tests/fixtures/abi-services` packs two independent native DLLs;
`tests/scenarios/abi-services.txt` verifies startup ordering and round trips, and
`tests/scenarios/abi-services-missing.txt` verifies a missing provider is skipped.

`moddedmc:content` v1 appends `on_random_tick(reg, block, callback)`.
Bind one callback per block already staged by the calling mod. The SDK uses
one Rust callback for the list of block IDs passed to `Content::on_random_tick`.
Pumpkin selects positions from ticking chunk sections using the world's random
tick speed. It dispatches custom samples in dimension order on the server
tick thread after the parallel world workers have joined. It skips samples
whose state has changed or whose chunk is no longer loaded.
Require the field with `Content::require_random_tick()` in `Mod::start`.

`moddedmc:entities` v1 appends combat and projectile operations after `age`.
All SDK calls check their individual field offsets and return `None` if an
older host lacks the field.
Require `EntitiesExtension::CombatEffects`, `Freezing` or `Projectiles`
at startup when the corresponding group is mandatory.

- `Entity::hurt_by(attacker, damage_type, amount)` uses the registered damage
  type with the loaded attacker as both direct and causing entity. Damage
  still passes through the engine's usual damage hooks and mitigation.
- `Entity::remove_effect(id)` removes an active vanilla status effect and
  reports whether it was present. Players use the player-specific removal path.
- `EntitiesExtension::EntityEvents` requires three appended fields:
  `broadcast_event(EntityRef, u8) -> bool` (byte 416) broadcasts the Java
  entity event to tracking players and the entity itself;
  `remove_all_effects(EntityRef) -> bool` (424) removes all active effects
  using native removal semantics, returning false if none were removed;
  `set_custom_name(EntityRef, Str) -> bool` (432) accepts a UTF-8 JSON text
  component (up to 64 KiB) and updates entity name metadata. Saved item stacks
  also preserve styled `minecraft:custom_name` components as vanilla NBT.
  Each SDK method returns `None` when the field is absent.
- `Entity::attribute_value(id)` reads the computed living attribute, including
  modifiers. Invalid IDs and nonliving entities return `None`.
- `Entity::ticks_frozen()` and `Entity::set_ticks_frozen(ticks)` read and write
  the vanilla frozen counter. The engine clamps it to its supported maximum
  and updates client metadata. Negative inputs are rejected.
- `Entities::spawn_projectile(world, kind, owner, target, position, velocity)`
  creates a supported vanilla projectile in a loaded chunk. A shulker bullet
  requires a loaded target; other projectile types ignore it. The return is
  the new entity ID. `Entity::projectile_owner()` returns the resolved owner
  ID or `None`.

- `Entity::can_freeze()` reads Pumpkin's existing vanilla freeze eligibility
  predicate, including entity type, mob overrides, spectator status and
  freeze-immune wearables. Require `EntitiesExtension::CanFreeze`; an older
  host returns `None`.
- `Entity::shulker_bullet_target()` returns the bullet's current loaded target
  ID, or `None` for other entities, missing targets or older hosts (ABI -1
  for absent targets). Require `EntitiesExtension::ShulkerBulletTarget`.

- `Entity::player_inventory_item(slot)` returns the namespaced ID of a
  nonempty stack; `Entity::player_inventory_stack(slot)` returns its Java
  named-root stack NBT. Slots 0–35 are the main inventory (0–8 hotbar),
  36–39 are armor (feet to head), and 40 is offhand. Both return None for
  empty or invalid slots, nonplayers and older hosts. Require
  `EntitiesExtension::PlayerInventory`. Each read is a snapshot of one slot.

- Require `EntitiesExtension::PlayerFlight` for fall-flying state and
  mod-controlled glide start/stop, a companion acceleration request (0 cancels,
  1–1200 ticks), and the server's selected hotbar slot (0–8), usable with the
  inventory ID and NBT queries above. A mod starting glide must renew it each
  tick and stop it when eligibility ends. This extension sets the
  `ENTITY_IN_WATER` and `ENTITY_ON_CLIMBABLE` flags from native movement contact.
  The companion applies the requested acceleration
  only while airborne and gliding. This extension also provides native
  translated-box collision testing, item cooldown readiness/start,
  held-item durability damage and attack-strength reset. Older hosts
  safely reject these calls.

`moddedmc:block_world` v1 appends `World::set_weather(mode, ticks)` and
`World::raycast(from, to, except)` after `can_see_sky`. Weather modes are
0 (clear), 1 (rain), and 2 (thunder), with positive durations in ticks.
Require `WorldExtension::WeatherControl` and `WorldExtension::Raycast`
respectively.
Raycast uses collider shapes without fluids and returns the first block
position and the nearest hittable entity before that block, excluding
the supplied optional entity ID. It rejects nonfinite/out-of-world endpoints
and rays longer than 256 blocks. A successful miss is `Some((None, None))`;
invalid input or a missing host field is `None`.
The engine treats unloaded blocks as air; the ray does not load chunks.

Current Pumpkin does not expose damage-event causing/direct entities or
outgoing last-hurt history. These require engine changes before
they can be exposed accurately. `MobAi::last_hurt_by` remains available
and returns the victim's tick count at the last attributed hit.

`moddedmc:block_world` v1 appends `raw_brightness(world, pos, sky_darken)`.
It returns the greater of block light and sky light minus the supplied darkening,
or -1 outside loaded positions. This query never loads a chunk. Require
`WorldExtension::RawBrightness` and call `World::raw_brightness`.

The fixture in `tests/fixtures/abi-random-tick` and scenario
`tests/scenarios/abi-random-tick.txt` check disabled ticking, a callback that
replaces its sampled block, and raw light in both loaded and unloaded chunks.

Three appended `moddedmc:block_world` v1 operations support saved tools and block interactions.

- `apply_block_transformer(world, origin, direction, tool)` traces a one-block outline ray
  from the center of a loaded position, ignoring fluids. On a hit it applies the saved
  tool's block transformer, including its world effects. The caller owns the saved stack.
  Require `WorldExtension::BlockTransformer`.
- `redstone_conductor(world, pos)` queries the loaded state's redstone conduction.
  It returns 1 for a conductor, 0 otherwise, and -1 when unloaded.
  Require `WorldExtension::RedstoneConductor`.
- `block_drops_with_player_actor(world, pos, tool, out)` rolls block loot with the saved
  tool and an unspawned player-typed loot actor. It uses the same stack sink as
  `block_drops` and leaves the block and world unchanged.
  Require `WorldExtension::PlayerActorBlockDrops`.

The appended `InteractionExtension::HeldTracker` exposes the player's sneak
state through `UseItem::sneaking` and permits a borrowed held stack's mod data
and lodestone target to be updated together through `Stack::set_lodestone`.
A target of None clears the tracker. Immediate item-use writeback checks the
original inventory slot and stack before committing; this is not a rollback
mechanism for arbitrary callback side effects.

`WorldExtension::ChestObstruction` exposes `World::chest_blocked`, using the
host's vanilla chest obstruction logic, including sitting cats. Unloaded
positions return None.

`WorldExtension::ContentEntityTransaction` provides `World::entity_snapshot`
and `World::compare_entity` for loaded custom block entities. A snapshot
contains the exact block state, live incarnation and full saved NBT. A
compare-and-replace rejects mismatched state, incarnation or NBT, preserving
the entity type and position. A successful update changes the live fields and
pending chunk record together. Incarnations identify live objects only and
must never be persisted as durable container IDs. These operations neither
convert vanilla block entities nor provide cross-file crash atomicity.

The published block-world v1 prefix includes `write_with_neighbor_shapes`
at byte 216 on 64-bit targets. Chest obstruction, guarded entity operations
and game time follow it. Rebuild packages compiled with the alternate
workstream layout before using them on main. The two layouts report the same
table size.
