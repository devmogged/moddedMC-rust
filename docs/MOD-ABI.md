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
| `moddedmc:commands` | `add(reg, name, description, op_level, nodes, count)` | Stage a root and its children during registration. The 1 to 128 child nodes name an earlier parent or the root. Nodes are literals or bounded integers. Executable nodes pass a tag to `ModApi::command`. The permission is `<mod id>:command.<name>`, with a default operator level from 0 to 4. |
| | `integer(source, name, out)` | Copy a parsed integer to the caller's output slot. False means the argument was absent. |
| | `feedback(source, text)` | Send command feedback using the source's feedback settings. |
| `moddedmc:lifecycle` | `on_stopping(reg, callback)` | Stage one callback per mod. The host runs it on a blocking thread and waits for it before world save. |

Chunk requests take chunk coordinates and return immediately. A false return means the request was rejected with no callback. For an accepted request the host returns the opaque context exactly once, possibly before submission returns, on a server runtime thread. Keep callbacks short. Chunky uses a coordinator thread per active dimension to plan work, receive callbacks, log progress and save task files. The server stops accepting chunk requests on shutdown and drains accepted work before it saves worlds.

The Rust wrappers live in `world`, `chunks`, `commands` and `lifecycle`. Logging and the mod data folder still use the ABI 1 base table. [Chunky](../mods/chunky) keeps its `port-chunky` id so existing task files migrate from `plugins/port-chunky` to `config/port-chunky`.

## Packaging

A `.mmc` is a zip with

- `mod.json`: `{"id": "wayshrine", "version": "0.2.0", "abi": 1}`
- `data/`, optional: the mod’s recipes, loot tables, tags, advancements and functions in datapack layout. The server loads it with every world as the built-in datapack `mod/<id>`, before the world’s own datapacks.
- `assets/`, optional: the mod’s models and textures, sent to players in the server’s resource pack. A port of a mod whose assets aren’t free to redistribute leaves them out; the server then reads them from a local copy of the original jar (`MODDEDMC_ASSET_JARS`, or a `cache/` folder above the server directory).
- one library per platform, named `<os>-<arch>.<ext>`: `windows-x86_64.dll`, `windows-aarch64.dll`, `linux-x86_64.so`, `linux-aarch64.so`, `macos-aarch64.dylib`, `macos-x86_64.dylib`.

`scripts/pack-mod.ps1` writes the manifest and the build for the current machine. Builds for other platforms (from CI or cross-compiling) go into the same zip under their names.

### Real content tables

| Interface | Function | Contract |
| --- | --- | --- |
| `moddedmc:content` v1 | `add_block(reg, definition)` | Stage the block, its item, its block entity name and an ordered list of states. The first state is the default. States provide exact property strings, a vanilla physics template, optional collision and outline boxes in sixteenths, model resource locations, and a display orientation. |
| | `state(selector)` | Resolve a block name or exact `namespace:path[key=value,...]` after registration. Missing definitions return `UNLOADED_STATE`. |
| | `on_tick(reg, callback)` | Call the mod once per loaded dimension in the server tick with queued state, neighbor and chunk lifecycle changes. The callback and its world handle are synchronous. |
| | `on_replaced(reg, callback)` | Report replacement of an owned block by another block type after the state write and before the old block entity is removed. The callback can copy the old entity NBT and must defer world writes to the mod tick. Moved replacements have their own event reason. Appended. |
| | `MOVED_STATE_CHANGED` | Event reason for a replacement made with `UPDATE_MOVED`, so a mod can detach a moved block (vanilla `isMoving`) instead of treating it as broken. |
| `moddedmc:items` v1 | `add_item(reg, definition)` | Stage an item with no block: its id, display name, the vanilla item it copies components from (stack size, food, tools), and its model. Stock clients see that vanilla item drawn with the model. Ids share one namespace with blocks. |
| `moddedmc:block_world` v1 | `read(world, positions, out, count)` | Read at most 4,096 positions without loading chunks. Unloaded positions return `UNLOADED_STATE`. |
| | `write(world, updates, count)` | Validate the batch before applying edits in input order. Each edit explicitly chooses neighbor and client notifications. `UPDATE_MOVED` is vanilla's move-by-piston and suppress-drops write: the replaced block entity is removed without side effects, so a container captured with `read_block_entity` doesn't scatter. Older hosts reject a batch using it. |
| | `command_world(source)` | Borrow the command source's world for the callback. |
| | `read_entity`, `write_entity` | Copy Java named-root NBT for a loaded custom block entity. The write keeps its registered id and position and updates the chunk's saved copy. Writes have a 64 KiB limit. |
| | `rotate(world, position, axis, rpm)` | Set a rotating model's signed RPM around its positive X, Y or Z axis. |
| | `has_neighbor_signal(world, position)` | Vanilla `Level.hasNeighborSignal`. False when unloaded. Appended after the first release: require `WorldExtension::NeighborSignal`. |
| | `destroy(world, position, drop)` | Vanilla `Level.destroyBlock`: break particles, loot when `drop`, then air or the block's water. Appended: require `WorldExtension::Destroy`. |
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
| | `water_evaporates(world, position)` | Read the world's 26.3 water evaporation setting for a loaded position. |
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
| | `stack_count`, `stack_shrink` | Read or shrink the held stack. |
| | `stack_data`, `set_stack_data` | Read or replace up to 1 KiB of mod bytes stored on the stack (`custom_data` key `moddedmc:data`). Empty removes them. |
| | `cooldown(player, item, ticks)` | Vanilla `ItemCooldowns.addCooldown`. |
| | `on_use_block(reg, callback)` | The block use extension, appended to the table (`InteractionExtension::BlockUse`). One callback per mod for its content blocks: vanilla `useItemOn` with the held stack (empty for an empty hand), then, when it returns `USE_TRY_EMPTY_HAND` for the main hand, `useWithoutItem` with an empty stack. It gets the position, state, face, hit point, sneaking, creative, the hand and which of the two it is. |
| | `on_player_will_destroy(reg, callback)` | Vanilla `Block.playerWillDestroy` for the mod's content blocks: called with the player, position, state and creative after the break is accepted, before drops and removal. Appended: require `InteractionExtension::PlayerWillDestroy`. |
| | `on_redstone(reg, signal, connects)` | Redstone output for the mod's content blocks. `connects(state, direction)` is vanilla `isSignalSource()` with NeoForge `canConnectRedstone(direction)` and decides where wire connects; `signal(world, pos, state, direction, direct)` is vanilla `getSignal`, or `getDirectSignal` when `direct`, called for every neighbour query as vanilla does, clamped to 15. Directions are vanilla's argument in order down, up, north, south, west, east. Block writes that change a state already notify neighbours, so wire and lamps update. Appended: require `InteractionExtension::Redstone`. |
| | `stack_read`, `stack_write` | The held stack as saved NBT, or replace it from saved NBT; empty clears it. |
| | `give(player, stack)` | Vanilla `Inventory.placeItemBackInInventory` for a saved stack: what doesn't fit drops at the player. Inside a use callback it merges into the used stack as vanilla's live held slot would, so read it again with `stack_read` afterwards. |
| `moddedmc:effects` v1 | `play_sound(world, position, sound, source, volume, pitch)` | Vanilla `Level.playSound(null, ...)` for players in range. Ids outside `minecraft:` play from the resource pack. `source` follows vanilla `SoundSource` order. |
| | `particles(world, particle, position, spread, speed, count)` | Vanilla `ServerLevel.sendParticles` for a particle type without options. |
| `moddedmc:resources` v1 | `list(directory, out)` | Vanilla `ResourceManager.listResources`: every file under `data/<namespace>/<directory>/` across mod data, the world's datapacks and Pumpkin's bundled packs, as sorted `namespace:directory/...` ids. Directory packs only. Vanilla cooking recipes, which Pumpkin compiles in, are listed and read under `recipe` as their vanilla JSON when no pack overrides them. |
| | `read(id, out)` | The highest-priority pack's bytes for an id, at most 4 MiB. |
| | `generation()` | 0 before datapacks first load, then a new value after each load, `/reload` included. Calls read the disk, so cache parsed data and reparse when it changes. |
| `moddedmc:tags` v1 | `values(registry, tag, out)` | A tag's entry ids by string, so content blocks and items count: Pumpkin's generated vanilla entries plus every loaded pack's, nested tags flattened, sorted. A pack's `replace` drops lower packs' and the generated vanilla entries, as in vanilla. |
| | `contains(registry, tag, id)` | Whether the id is in the tag. Cache results per `moddedmc:resources` generation. |
| `moddedmc:containers` v1 | `slots(world, position, face)` | NeoForge `Capabilities.ItemHandler.BLOCK` for the vanilla container at a position (what a hopper sees, the composter included), as `InvWrapper` or, for a sided container, `SidedInvWrapper` for that face. `NO_FACE` is the null side. -1 when there is none. |
| | `stack`, `slot_limit` | `getStackInSlot` as saved NBT, `getSlotLimit` (brewing stand bottles 1). |
| | `insert(..., slot, stack, simulate, out)`, `insert_stacked(..., stack, simulate, out)` | `insertItem` and `ItemHandlerHelper.insertItemStacked`; the remainder goes to `out`. Includes NeoForge's one-bucket furnace fuel limit and `VanillaHopperItemHandler`'s cooldown when filling an empty hopper. |
| | `extract(..., slot, amount, simulate, out)` | `extractItem`, honouring `canTakeItemThroughFace` for a side. |
| `moddedmc:block_displays` v1 | `spawn(world, state, display)`, `update`, `set_state`, `remove` | Block displays that are never saved, for blocks that aren't in the world (a moving contraption). The display gives the position, the model translation, a left rotation quaternion, per-axis scale, the transformation blend in ticks (`interpolation_duration`, restarted on each update), the position interpolation (`teleport_duration`) and a brightness override. Content states show as their carrier on stock clients. |
| `moddedmc:item_stacks` v1 | `entity_stack`, `set_entity_stack` | Read an item entity's stack, or replace it; an empty stack discards the entity. Stacks are vanilla saved `ItemStack` NBT (`id`, `count`, `components`) as Java named-root binary, at most 64 KiB, so mods can keep them in their own block entity data. |
| | `fuel(stack, remainder)` | Regular furnace burn time from the stack's effective cooking fuel component. Returns zero for a non-fuel or invalid stack. The sink gets the crafting remainder as saved stack NBT when present. Require the appended entry with `StacksExtension::Fuel`. |
| | `spawn(world, position, motion, stack)` | Vanilla `new ItemEntity`, `setDeltaMovement`, `setDefaultPickUpDelay` and `addFreshEntity`, firing the plugin item spawn event like block drops. Returns the entity id. |
| | `info(stack, out)` | Count, vanilla `getMaxStackSize`, and whether the item places a block. |
| | `component(stack, component, out)` | Vanilla `ItemStack.get(component)`: the patch over the item defaults, as `{value: <component>}` named-root NBT, an empty root for unit components or ones Pumpkin doesn't encode yet. False when absent. Appended: require `StacksExtension::Component`. |
| `moddedmc:displays` v1 | `spawn_item(world, stack, display)` | An item display that is never saved, for things that aren't entities (items on a belt). The display gives the position, a left rotation quaternion, a uniform scale, the `ItemDisplayContext` and the client's position interpolation in ticks. Returns its id. |
| | `update`, `set_item`, `remove` | Move or restyle it, change its stack, or take it down. `update` returns false once the display is gone (its chunk unloaded); spawn a new one then. |
| `moddedmc:mobs` v1 | `add_mob(reg, definition)` | Stage a mob type: its id, the vanilla living type that carries it (`minecraft:ravager`), and base attribute values set when it spawns (then saved like any attribute). Stock clients see the carrier. |
| | `spawn(world, kind, position)` | Spawn a registered mob type, or a vanilla mob by id. Returns its entity id, or -1. A registered mob starts at full health for its `max_health`. |
| | `kind(entity)` | The registered type of a mod mob, or empty. It is kept in the entity's custom data (`data.moddedmc.mob`), so it survives restarts; a mob whose type is no longer registered reads as empty. |
| | `state(entity, out)`, `set_state(entity, bytes)` | The mod's own bytes on a mod mob, at most 64 KiB, saved with it (`data.moddedmc.state`). False for anything that isn't a mod mob. |
| `moddedmc:mob_ai` v1 | `add_goal(reg, definition)` | Stage a goal for a registered mod mob type: a key, a vanilla priority, `goal_control` bits (`MOVE`, `LOOK`, `JUMP`, `TARGET`) and a callback. Every mob of the type gets it as it joins a world, spawned or loaded. The callback is asked each tick with a `goal_phase` (`CAN_START`, `CONTINUE`, `START`, `STOP`, `TICK`), the world and the mob; only the answer to `CAN_START` and `CONTINUE` counts. Goals with `TARGET` run in the target selector. |
| | `replace_goals(reg, kind)` | Mobs of the type drop their carrier's own goals and targets before getting the mod's. |
| | `walk_to(mob, target, speed)`, `stop(mob)`, `navigating(mob)` | Path to a position at a multiple of the movement speed, drop the path, or ask whether one is being followed. |
| | `look_at(mob, position)` | Turn the head toward a position this tick. |
| | `set_target(world, mob, id)`, `target(mob)` | Set or clear (-1) the attack target by entity id, or read it (-1 when none). |

Require the tables from `Mod::start` and register definitions in `Mod::register`. Requesting `moddedmc:content` or `moddedmc:items` during startup moves that mod's registration before world load. Legacy mods continue to register on the first tick. State numbers are runtime handles. Store names and properties in mod-owned data. Mods with a placement hook can choose one of the placing block's states from the clicked face, look direction, sneak state and loaded neighbors.

The Rust SDK exposes them through `moddedmc_api::content`, `moddedmc_api::items`, `moddedmc_api::stacks`, `moddedmc_api::interaction`, `moddedmc_api::effects`, `moddedmc_api::mobs` and `moddedmc_api::mob_ai`. See `mods/create` for registration, placement, loading saved NBT and processing batches, and `mods/saints-dragons` for mob types.

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
