# Decisions

Project-level decisions and why. Agents and people working on any branch read this first. To change a decision, change it here first, with the reason, then change the code.

## 2026-11-19: Every mod is a drop-in file, big ports included

**Decision.** All mods, including large Rust ports (Create, Saints Dragons, L_Ender's Cataclysm), ship as `.mmc` files that server owners drop into `mods/` and remove by deleting, with a restart and no server rebuild. Fabric `.jar` files stay drop-in through the converter.

**How.** Mods talk to the server through the stable C ABI (`api/src/abi.rs`). Engine features reach mods as **named, versioned interfaces** (`moddedmc:world/1`, `moddedmc:blocks/1`, ...) requested with `HostApi::query_interface`. Each interface is versioned on its own and older versions keep being served, so the ABI never has to mirror the whole engine at once. The interfaces grow by porting real mods and adding what they need. Heavy logic (Create's kinetic networks, dragon AI, boss fights) lives inside the mod; host calls are batched.

**Why not compiled-in packs** (the `platform/static-packs` branch: one server binary per pack, mods use Pumpkin's Rust types, no ABI):

- Changing the mod list means rebuilding the server: 17 to 44 minutes on our Windows machine, every time. Server owners expect drop-in.
- The speed argument is small: crossing the boundary cost 0.04–0.05 ms per tick with 10,000 ticking blocks. The real costs are in the engine (saving block state was 2.2–2.5 ms).
- Mods written against Pumpkin's Rust types break whenever we merge upstream Pumpkin.
- Linking mods with other licenses into one GPL binary is a distribution problem for some planned ports.

**Reviewed by** GPT-6.2 Sol, which recommended the modular C ABI for large ports and keeping version-locked Rust `dylib` mods only as an experiment (exact-build matching, no cross-library LTO, duplicated `std`/Tokio/tracing, Windows export limits).

**Taken from the static-packs branch:**

- Engine changes as commits on our fork `devmogged/Pumpkin` instead of `patches/pumpkin.patch` (to do; until then the patch rules in AGENTS.md apply).
- The differential harness against vanilla 26.3 as the definition of done.
- Real modded registries (modded block states above vanilla's ids, per-world id maps, per-connection translation, generated resource pack): this becomes `moddedmc:blocks/1`.
- Cross-compiling for the Raspberry Pi, and the performance targets.

**Removing a large mod** must be reversible: snapshot before the first write without it, keep its registry ids reserved and its blocks, entities and items as inert placeholders that hold their data, restore them when the same mod comes back, and refuse to start the world writable when a missing mod makes it unsafe. Purging is an explicit offline step.

**First proof.** One `.mmc` with a Create-style drivetrain (motor, shaft, unequal gears, clutch, load) across chunk borders: speed, direction, overload and recovery match Create through network splits and merges, chunk unloads and a restart; a stock client can place and see it; removing and re-adding the file restores it; the same file runs on two server builds; startup and p99 tick time are measured on the Pi 5.

**2026-11-19: how `blocks/1` blocks are stored and shown.** Modded state ids exist only at runtime: they are numbered after vanilla's at startup, worlds save names, and each client connection gets the vanilla look-alike's id. So there is no per-world id map to keep in sync, and adding or removing a mod never renumbers a saved world. Physics come from the look-alike, because a stock client predicts movement and breaking from what it sees; custom shapes need the resource pack or the companion first. Engine changes for this live in `patches/pumpkin.patch` until the move to the fork.

**Steps.**

1. `query_interface` and `moddedmc:world/1` (batched block reads and writes). Done on branch `abi/interfaces`.
2. `moddedmc:blocks/1`: real modded blocks with states, items and interactions. Done on branch `abi/interfaces` (see below); the resource pack (mods' `assets/`, merged, served and offered on join) is done too. Custom shapes are not possible for a stock client, which predicts collision from the vanilla block it sees; they need the companion.
   Removing a mod keeps its blocks as inert placeholders, snapshots the world before the first start without them, and refuses to start if modded blocks could not be kept. Purging stays an explicit offline step (delete the lines from `config/moddedmc/blocks.txt`).
3. Block events: neighbor (with redstone power) and scheduled ticks for `blocks/1` blocks, delivered in order at the start of each tick. Chunk load events (`Load`, also after a restart) too. Done on branch `abi/interfaces`.
4. The drivetrain proof: `mods/drivetrain` and `tests/scenarios/drivetrain.txt` (motor, shafts across a chunk border, small and large cogs at 2:1, clutch, load, overstress and recovery, split and repair, chunk reload, restart, removing and re-adding the mod). Done on branch `abi/interfaces`, with these gaps: speeds follow Create's rules but were not compared against a running Create; a stock client was not tried; nothing was measured on a Pi 5. The same `drivetrain.mmc` passed on two server builds (before and after the chunk-load change).
5. `entities/1`, `menus/1`, `worldgen/1` as the dragon and boss ports need them. `entities/1` (spawn, find, move, name, remove, and player use and attack events; `mods/wisp`, `tests/scenarios/abi-entities.txt`) and world generation (`ModApi::chunk_generated`, once per chunk and mod; `mods/ores`, `tests/scenarios/worldgen.txt`) are done on branch `abi/interfaces`. Inventory menus are done too (`menus/1`; `mods/backpack`, `tests/scenarios/menus.txt`). Machine screens, other entity events (damage from other sources, death) and AI hooks, structures and biomes wait for a port to say what it needs: designing them ahead of a real user would only guess.
