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
- To remove a mod, delete its file and restart. `mods/.cache/` can be deleted at any time; it is rebuilt. Blocks of a removed mod stay in your worlds as they were (inert, listed in `config/moddedmc/blocks.txt`) and work again when you put the mod back. Keep `config/moddedmc/`: it records which modded blocks your worlds contain. The first start without a mod's blocks copies `world/` to `backups/moddedmc-<time>/` first. If the server cannot set up modded blocks at all while your worlds contain some, it refuses to start rather than lose them.
- **Resource pack.** Mods can carry textures, models, sounds and translations. The server merges them into one pack, serves it over HTTP (port 25580 by default) and offers it to every player who joins. Set `url=` in `config/moddedmc/pack.txt` to an address your players can reach (and `bind=` to where the server should listen); the default only works on the server's own machine.
- A mod's own files live in `config/<mod id>/`. Fabric mods use `config/` as on a Fabric server.
- The server log shows each mod it loaded (`moddedmc: loaded wayshrine 0.2.0 from mods\wayshrine.mmc`) and why any mod was skipped. One broken mod never stops the others or the server.

A mod built for this server's mod ABI (currently **1**) runs on any server version with the same ABI. Mods are trusted code: only install mods you trust, as with any Minecraft server plugin.

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
| `Mod::register` | Once, on the first server tick: `add_block_entity`, `add_block`, `add_command`. |
| `Mod::tick` | Due block entities of one dimension; `set_state(index, bytes)` saves a new state. |
| `Mod::use_block`, `place_block`, `break_block` | A player used, placed or broke one of your blocks. |
| `Mod::command` | One of your commands ran. `reply` answers the player or the console. |
| `Mod::menu_click` | A player picked a button of a menu (`Player::show_menu`). Every mod sees every click: check the action id is yours. |
| `Mod::define_blocks` | Once, before any world loads: `blocks.block("shaft:shaft", &[("axis", &["x", "y", "z"])], &[look-alikes])` defines a real modded block (`moddedmc:blocks/1`). |
| `Mod::block_event` | One of your `blocks/1` blocks was used, placed, broken, ticked, or a neighbor changed. Return true from a use to skip the look-alike's own interaction. |
| `Mod::chunk_generated` | World generation: once per chunk, the first time it loads with your mod installed (new chunks, and old ones your mod has not seen). Place ores and features with `world`, inside that chunk. The server records decorated chunks in `config/moddedmc/decorated/<mod>/`. |
| `blocks::get()` | `schedule_tick(dimension, pos, delay)` and `is_powered(dimension, pos)` for `blocks/1` blocks. |
| `entities::get()` | `moddedmc:entities/1`: `spawn` a vanilla entity type as a creature's look-alike, then `position`, `move_to`, `set_name`, `remove` by its id, `set_ai` to switch a mob's vanilla AI off so only your mod moves it, and `walk_to` to send a mob along a path and keep it there until `release`. Ids survive restarts (the world saves the entity): store them. |
| `Mod::entity_event` | A player right-clicked (`Use`) or attacked (`Attack`) an entity. Every mod sees every one: act only on ids you spawned. Return true to cancel the vanilla interaction or the damage. |
| `menus::get()` | `moddedmc:menus/1`: `open` a chest screen of 1 to 6 rows filled with your items; players move items in and out as in vanilla. An item is opaque bytes (vanilla item NBT, every component kept): store them as they are, and use `item(id, count)` and `describe(bytes)` to make and read them. `open_machine` opens a furnace, blast furnace or smoker screen (3 slots the mod gives meaning to) and `set_property` drives its fuel and progress bars. |
| `Definitions::biome` | In `define_blocks`: a biome of your own (`frost:glacier`). The server treats it like a vanilla look-alike (mob spawns, rain or snow); clients draw it from your JSON, the body of a vanilla `worldgen/biome` file (`effects` colors, `attributes` such as `minecraft:visual/sky_color`). At most 61 modded biomes fit. |
| `biomes::get()` | `moddedmc:biomes/1`: `set` the biome of a 4x4x4 cell (e.g. from `chunk_generated`; players see it from the next tick) and read it with `at`. |
| `Mod::menu_closed` | A player closed a `menus/1` menu: its key and one item per slot. Every mod sees every close: start your keys with your mod id and act only on those. Opening a menu from here fails. |
| `Player` | `name`, `dimension`, `position`, `send_message`, `action_bar`, `give_block`, `teleport`, `show_menu`. Valid only during the call that handed it to you; store the name, not the player. |
| free functions | `info`, `warn`, `error` (server log), `data_dir()` (`config/<id>/`), `tick_count()`, `dimensions()`. |
| `world::get()` | The `moddedmc:world/1` interface, if the server has it: `get_blocks`, `set_blocks` (batched, any vanilla block state), `state_id("minecraft:oak_log[axis=x]")`, `state_name(id)`. State ids are valid for this Minecraft version only: keep them in memory, save names. |

Rules:

- The mod id is the package name (or `export_mod!(M, id = "...")`): lowercase letters, digits, `_`, `-`, `.`. It names the data folder. Block ids are `namespace:path` and may use any namespace except `minecraft`.
- Calls can come from several threads at once, so the mod type must be `Send + Sync`.
- A panic is caught by the SDK, logged, and treated as "did nothing". It never crashes the server.
- Block entity state is opaque bytes, at most 64 KiB, saved with the chunk. Bump `state_version` when its layout changes; the server will not hand old-layout state to the new code.
- You do not need the server's source, compiler version or build to build a mod. Any Rust toolchain that builds `api/` works.

`mods/counter` (a ticking block), `mods/wayshrine` (blocks, a menu, teleport, saved data), `mods/pillar` (the world interface), `mods/shaft` (a `blocks/1` block), `mods/drivetrain` (a Create-style drivetrain: networks, gears, clutch, stress), `mods/wisp` (a creature through `entities/1`), `mods/backpack` (an inventory through `menus/1`), `mods/frost` (a biome through `biomes/1`) and `mods/ores` (zinc ore through `chunk_generated`) are complete examples.

### Modded blocks (`blocks/1`)

A `blocks/1` block is a real block: its states have their own ids, the world saves them by name, and `world::get()` reads and writes them like vanilla states (`state_id("shaft:shaft[axis=x]")`). A stock client has no textures for it, so each state names a vanilla **look-alike** that players see, and the block takes the look-alike's shape, collision, hardness and light (the client predicts those from what it sees). Pick look-alikes that behave like your block and that players rarely use, and give each state a different one if players should see the difference.

- Define blocks in `define_blocks`; it runs before any world loads, and an error disables all of that mod's blocks.
- `Player::give_block("shaft:shaft")` gives the look-alike's item, tagged. Placing it places your block in the state whose look-alike matches what the player placed (so `facing` and `axis` carry over), then sends `Place`.
- `Use`, `Place`, `Break`, `Tick`, `Neighbor` and `Load` arrive in `block_event`. `Load` comes for each of your blocks when its chunk loads, including after a restart. Breaking drops the item. `Tick` comes when you asked for it with `blocks::get()?.schedule_tick(...)`; scheduled ticks are not saved, so schedule again after a restart (keep the positions you need in your data folder, as `mods/drivetrain` does with its motors). `Neighbor` comes once per block and tick when a block next to it changed, including redstone power; read the power with `is_powered`.
- Events arrive at the start of a server tick, never in the middle of a world update, so it is safe to read and write blocks from them.
- Property names and values use lowercase letters, digits, `_`, `-` and `.`.
- Put resource pack files in your crate's `assets/` folder (`assets/<namespace>/...`, as in any resource pack); `pack-mod.ps1` packs them into the `.mmc` and the server merges every mod's assets into one pack for players. A file another mod also has is replaced by the later mod in file-name order, with a warning. To retexture a look-alike, override that vanilla block's model or blockstate; that changes the vanilla block too, so prefer look-alikes players rarely see.

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
- New host functions and new mod callbacks are appended to the end of their table, and `size` (the table's byte size) grows. An older mod runs on a newer server. A newer mod runs on an older server as long as that server has ABI 1.0's functions; the SDK checks `size` before using anything newer and treats a missing function as "not offered".
- Bigger features come as **named interfaces** (since 1.1): `HostApi::query_interface("moddedmc:world/1")` returns a table, or null if the server does not offer it. Each interface is versioned on its own: a breaking change to one ships as `.../2` next to `.../1`, which the server keeps offering, so mods built against `/1` keep working. Every table starts with its own `size`.

Interfaces:

| Name | Since | What |
| --- | --- | --- |
| `moddedmc:world/1` | 1.1 | Batched block reads and writes by state id (vanilla and `blocks/1`); state names to ids and back. |
| `moddedmc:blocks/1` | 1.1 | `define_block` during `ModApi::define_blocks`; `schedule_tick`, `is_powered`. Events come through `ModApi::block_event`. |
| `moddedmc:entities/1` | 1.1 | Spawn, find, move, name and remove entities by UUID. Uses and attacks arrive through `ModApi::entity_event` (1.1). |
| `moddedmc:biomes/1` | 1.1 | Biomes: defined in `define_blocks`, set and read per 4x4x4 cell. |
| `moddedmc:menus/1` | 1.1 | Chest-style inventories; contents come back through `ModApi::menu_closed` (1.1). |

World generation is the `ModApi::chunk_generated` callback (1.1) rather than an interface table: the mod places blocks itself through `world/1`.

Planned (see [DECISIONS.md](DECISIONS.md)): more entity events (damage from other sources, death), custom AI goals and targeting, structure helpers for world generation, per-slot rules for machine screens (which item may go where), each when a real port needs it. Button menus are `Player::show_menu` (dialogs); inventories are `menus/1`.

Packaging (`.mmc`): a zip with

- `mod.json`: `{"id": "wayshrine", "version": "0.2.0", "abi": 1}`
- one library per platform, named `<os>-<arch>.<ext>`: `windows-x86_64.dll`, `windows-aarch64.dll`, `linux-x86_64.so`, `linux-aarch64.so`, `macos-aarch64.dylib`, `macos-x86_64.dylib`.

`scripts/pack-mod.ps1` writes the manifest and the build for the current machine. Builds for other platforms (from CI or cross-compiling) go into the same zip under their names.
