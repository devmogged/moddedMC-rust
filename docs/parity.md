# Parity with vanilla 26.3 (E1, first pass)

What our Pumpkin fork has against vanilla 26.3, from the fork at `engine/pumpkin` and the decompiled 26.3 server (`D:\mc-ref\26.3\src`), 2026-11-19. This pass measures **breadth**: whether a system exists at all. Whether it behaves like vanilla is the job of the differential harness (F2); every "present" below still needs harness scenarios before it counts as done.

## Summary

| Area | Breadth | Known gaps and weak spots |
| --- | --- | --- |
| Commands | 90 command files against vanilla's 83 | Missing `version`, `jfr`, `perf`. The rest of vanilla's list are debug-only, client-only (`publish`, `unpublish`) or under another name (`listplayers` is `list`, `setplayeridletimeout` is `setidletimeout`). |
| Entities | 159 of 161 entity types map to an implementation (`entity/type.rs`); the other two (player, fishing bobber) are created elsewhere | AI uses goals (about 70) and Brain behaviours. Open TODOs: melee attack and target tracking don't use paths yet, hoglin/piglin retreat counts aren't filled, ender dragon parts aren't added to the world, mob equipment enchantment processing, spawn reasons. |
| Blocks | About 110 behaviour files and 50 block entities, including redstone, pistons, hoppers, sculk, vaults, trial spawners, crafter | Pistons have open TODOs (extra checks, `pushed_block_state`). Hoppers don't pull item entities on contact (`HopperBlock.entityInside`). Spawners and trial spawners ignore `SpawnData` NBT. Loot rolls don't use the world-seeded `random_sequence`. |
| Items | About 50 item behaviours (tools, buckets, bows, crossbow, trident, mace, spear, maps, fishing rod, bundles, minecarts, boats) | Armor stand equipment slots and drops. |
| Inventories | Every vanilla screen: crafting, furnaces, anvil, enchanting, brewing, smithing, grindstone, loom, stonecutter, cartography, beacon, lectern, crafter, merchant, horse | Fidelity unknown. |
| Worldgen structures | All 16 vanilla structure types | Checked by Pumpkin's own chunk-dump tests for some seeds; not all structures are covered there. |
| Worldgen features | All 58 vanilla feature types are generated | `sequence` and `template` were no-ops until 2026-11-19; desert wells, deep dark and ancient city sculk, sulfur pools and sulfur springs now place. Templates with several palettes always place the first one. |
| World | Lighting, ticking, weather, forced chunks, entity tracking | 28 TODOs in `world/`. |
| Datapacks | Vanilla data is generated at build time. Recipes, loot tables, tags, advancements and functions load at runtime from `world/datapacks` and from each mod’s `data/` | Loading custom worldgen from datapacks at runtime is E2. Datapack tags add blocks and items to gameplay tag checks but can’t remove generated entries (`replace` is ignored), and a generated tag that nests a datapack tag doesn’t pick up its additions. |

## Next, in order

1. Done: `sequence` and `template` features, with the `random_chance` and `randomly_selected` placements they use.
2. Done: the `cuboid` placement (`warm_ocean_vegetation`).
3. F2 harness scenarios for the areas players notice first: redstone (pistons, hoppers, repeaters), mob spawning and AI pathing, containers, then worldgen against vanilla chunk dumps.
4. The TODO hotspots above, each checked against the decompiled vanilla method named in its comment.

## How this was measured

Commands: vanilla `server/commands/*Command.java` against `command/commands/*.rs`. Entities: `assets/entities.json` against the `EntityType` arms in `entity/type.rs`. Features: the `register` calls in vanilla `FeatureTypes.java` against the cases in `tools/pumpkin-codegen/src/configured_feature.rs`, then which of vanilla's `worldgen/feature/*.json` use the unhandled types. Structures: vanilla `levelgen/structure/structures` against `generation/structure/structures`. TODOs: `todo`, `unimplemented` and `not implemented` comments in `pumpkin-core/src`.
