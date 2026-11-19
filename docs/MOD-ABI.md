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
| `moddedmc:items` v1 | `add_item(reg, definition)` | Stage an item with no block: its id, display name, the vanilla item it copies components from (stack size, food, tools), and its model. Stock clients see that vanilla item drawn with the model. Ids share one namespace with blocks. |
| `moddedmc:block_world` v1 | `read(world, positions, out, count)` | Read at most 4,096 positions without loading chunks. Unloaded positions return `UNLOADED_STATE`. |
| | `write(world, updates, count)` | Validate the batch before applying edits in input order. Each edit explicitly chooses neighbor and client notifications. |
| | `command_world(source)` | Borrow the command source's world for the callback. |
| | `read_entity`, `write_entity` | Copy Java named-root NBT for a loaded custom block entity. The write keeps its registered id and position and updates the chunk's saved copy. Writes have a 64 KiB limit. |
| | `rotate(world, position, axis, rpm)` | Set a rotating model's signed RPM around its positive X, Y or Z axis. |
| | `has_neighbor_signal(world, position)` | Vanilla `Level.hasNeighborSignal`. False when unloaded. Appended after the first release: require `WorldExtension::NeighborSignal`. |
| | `destroy(world, position, drop)` | Vanilla `Level.destroyBlock`: break particles, loot when `drop`, then air or the block's water. Appended: require `WorldExtension::Destroy`. |
| | `face_sturdy(world, position, face)` | Vanilla `BlockState.isFaceSturdy`. False when unloaded. Appended: require `WorldExtension::FaceSturdy`. |
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
| `moddedmc:interaction` v1 | `on_use_on_block(reg, callback)` | Stage one callback per mod, called like vanilla `Item.useOn` for any of the mod's content items, block items included. It gets the world, the player (an entity reference), the held stack and the context: item id, clicked position and face, hit point, sneaking and creative. `PASS` lets the server place the item's block as usual. |
| | `stack_count`, `stack_shrink` | Read or shrink the held stack. |
| | `stack_data`, `set_stack_data` | Read or replace up to 1 KiB of mod bytes stored on the stack (`custom_data` key `moddedmc:data`). Empty removes them. |
| | `cooldown(player, item, ticks)` | Vanilla `ItemCooldowns.addCooldown`. |
| | `on_use_block(reg, callback)` | The block use extension, appended to the table (`InteractionExtension::BlockUse`). One callback per mod for its content blocks: vanilla `useItemOn` with the held stack (empty for an empty hand), then, when it returns `USE_TRY_EMPTY_HAND` for the main hand, `useWithoutItem` with an empty stack. It gets the position, state, face, hit point, sneaking, creative, the hand and which of the two it is. |
| | `stack_read`, `stack_write` | The held stack as saved NBT, or replace it from saved NBT; empty clears it. |
| | `give(player, stack)` | Vanilla `Inventory.placeItemBackInInventory` for a saved stack: what doesn't fit drops at the player. |
| `moddedmc:effects` v1 | `play_sound(world, position, sound, source, volume, pitch)` | Vanilla `Level.playSound(null, ...)` for players in range. Ids outside `minecraft:` play from the resource pack. `source` follows vanilla `SoundSource` order. |
| | `particles(world, particle, position, spread, speed, count)` | Vanilla `ServerLevel.sendParticles` for a particle type without options. |
| `moddedmc:resources` v1 | `list(directory, out)` | Vanilla `ResourceManager.listResources`: every file under `data/<namespace>/<directory>/` across mod data, the world's datapacks and Pumpkin's bundled packs, as sorted `namespace:directory/...` ids. Directory packs only. |
| | `read(id, out)` | The highest-priority pack's bytes for an id, at most 4 MiB. |
| | `generation()` | 0 before datapacks first load, then a new value after each load, `/reload` included. Calls read the disk, so cache parsed data and reparse when it changes. |
| `moddedmc:item_stacks` v1 | `entity_stack`, `set_entity_stack` | Read an item entity's stack, or replace it; an empty stack discards the entity. Stacks are vanilla saved `ItemStack` NBT (`id`, `count`, `components`) as Java named-root binary, at most 64 KiB, so mods can keep them in their own block entity data. |
| | `spawn(world, position, motion, stack)` | Vanilla `new ItemEntity`, `setDeltaMovement`, `setDefaultPickUpDelay` and `addFreshEntity`, firing the plugin item spawn event like block drops. Returns the entity id. |
| | `info(stack, out)` | Count, vanilla `getMaxStackSize`, and whether the item places a block. |
| `moddedmc:displays` v1 | `spawn_item(world, stack, display)` | An item display that is never saved, for things that aren't entities (items on a belt). The display gives the position, a left rotation quaternion, a uniform scale, the `ItemDisplayContext` and the client's position interpolation in ticks. Returns its id. |
| | `update`, `set_item`, `remove` | Move or restyle it, change its stack, or take it down. `update` returns false once the display is gone (its chunk unloaded); spawn a new one then. |

Require the tables from `Mod::start` and register definitions in `Mod::register`. Requesting `moddedmc:content` or `moddedmc:items` during startup moves that mod's registration before world load. Legacy mods continue to register on the first tick. State numbers are runtime handles. Store names and properties in mod-owned data. Mods with a placement hook can choose one of the placing block's states from the clicked face, look direction, sneak state and loaded neighbors.

The Rust SDK exposes them through `moddedmc_api::content`, `moddedmc_api::items`, `moddedmc_api::stacks`, `moddedmc_api::interaction` and `moddedmc_api::effects`. See `mods/create` for registration, placement, loading saved NBT and processing batches.
