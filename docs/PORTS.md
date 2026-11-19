# Rust mod ports

Every port ships as a `.mmc` in the server's `mods/` folder. Add or remove the file and restart. A pack contains the same stock server binary for its target, plus its own set of mod files.

## Build a port

Run `scripts/new-mod.ps1 -Id <id>`. It creates `mods/<id>`, a crate that already builds and runs: one block with its model, texture, recipe, loot table and mineable tag, and `tests/scenarios/<id>.txt`, which checks it on a fresh server. [Create](../mods/create) and [Chunky](../mods/chunky) show larger mods. The SDK is in [api](../api), and its C tables are defined in [MOD-ABI.md](MOD-ABI.md).

1. Require the named interface versions the port uses in `Mod::start`. A server that lacks a required version logs its name and skips the mod before worlds load.
2. Keep mechanics in the mod. Create owns its kinetic networks, and each boss or dragon mod owns its AI. The host exposes operations the engine can perform safely. Plan new operations in [INTERFACES-PLAN.md](INTERFACES-PLAN.md).
3. Copy the original mod’s data from its jar into `mods/<id>/resources/data/` unchanged: its own `data/<namespace>/`, plus the vanilla tag files it ships (`data/minecraft/tags/blocks/mineable/*.json` sets which tool mines each block). Old (`recipes`, `loot_tables`, `tags/blocks`) and new (`recipe`, `loot_table`, `tags/block`) folder names both load, and Forge 1.20 tags such as `#forge:nuggets/iron` resolve as their shared `c:` tags. Tag entries for blocks and items the port doesn’t register yet are skipped; leave out recipes that need them. A block’s drops come from `<namespace>:blocks/<path>`, as in vanilla.
4. Build with `scripts/pack-mod.ps1 -Crate <name>` or put the crate's path in a [pack manifest](PACKS.md). A pack can also use a prebuilt `.mmc`.

## Where each part of a Java mod goes

Read the original's source next to this table. Keep its ids, block state property names and NBT field names, so saved data and the harness line up on both sides.

| In the Forge or Fabric mod | In the port |
| --- | --- |
| Blocks from a `DeferredRegister` or `Registry.register` | `content.add_block` in `Mod::register`. List every state with its exact property string, such as `axis=x,waterlogged=false`. `physical` names the vanilla block whose hardness, sounds and collision the state uses; `shape` overrides the collision and outline boxes. |
| The block's item | Registered with the block. `item_model` names its model. |
| Items without a block (ingots, alloys, tools) | `items.add_item`. `carrier` names the vanilla item whose stack size, food and tool components it copies. |
| Blockstate JSON and block models | `model` per state, with `x` and `y` rotation as in the blockstate file. Parts that spin go in `rotating_model`, driven by `World::rotate`. |
| Models and textures | Free to redistribute: copy `assets/<namespace>/` into `resources/assets/`. Not free (Create's are All Rights Reserved): leave them out, and the server reads them from a local copy of the original jar. |
| `getStateForPlacement` | The placement hook passed to `add_block`. It gets the clicked face, look direction and sneak state. |
| Block entity class and its `saveAdditional`/`load` | The `entity` id on the block, plus `World::entity` and `World::set_entity`, which copy the block entity's NBT. Keep the original field names. |
| `tick`, `neighborChanged`, `onPlace`, `onRemove` | `content.on_tick`. It runs once per dimension per tick with the queued state, neighbor and chunk load or unload events. |
| `level.setBlock` and reads | Batched `read` and `write` on `World`, which never load chunks. Each write picks its neighbor and client updates. |
| Recipes, loot tables and tags | Copy `data/` unchanged (step 3 above). |
| Brigadier commands | `moddedmc:commands`. |
| Server stopping and save hooks | `moddedmc:lifecycle`. |
| Client-only code (renderers, screens, key binds) | Not ported. The server sends models and textures in its resource pack. |
| Entities, menus, worldgen features, custom recipe types, network packets | Not available yet. Add the operation the port needs to [INTERFACES-PLAN.md](INTERFACES-PLAN.md) instead of working around it. |

A new mechanic may need a server release that adds a generic engine hook and a named interface. Existing mods use the versions they were built against. This model gives up direct engine access and cross-mod compiler optimization.

The server's `pack` profile is optimized and incremental for local work. Use `release` with fat LTO for deployment. Each mod builds independently of the server.

## Measure against the original

Use the Java mod with the same seed and settings. Record throughput, MSPT and peak RSS where they apply. Ports should aim for several times the throughput and a fraction of the RAM, with outcomes checked against the original mod. Run `scripts/pregen-bench.ps1 -Radius 1000 -Runs 3` for Chunky. Results are in [BACKLOG.md](BACKLOG.md).
