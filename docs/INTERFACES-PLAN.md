# Interface plan for the next ports

Release each table with the first port that needs it and pin its version. Preserve [CONTENT.md](CONTENT.md)'s append-only per-world registry ids, vanilla view and companion view.

## 1. Create

- **Real block and item registry.** Register namespaced blocks, states and items before world load. Store the F4 ids in chunk palettes and inventories. Start with shaft and cogwheel states, axis properties, shapes and placement. Translate them through the vanilla resource pack and later through the companion.
- **Block entities and batched world access.** Add persistent block entity state, ordered tick and neighbor callbacks, bulk reads of positions and state ids, and bounded bulk writes with explicit update flags. Keep network discovery, stress and rotation in the mod. Compare placement, removal, speed and direction with the original shaft-and-cogwheel rig.
- **Rendering and later mechanics.** Send versioned kinetic state to the companion for rotation and belts. Add engine operations for contraption assembly, transport, fluids, inventories and trains as those slices land. Put ore placement through the worldgen path when needed. [Kinetic block entity](https://github.com/Creators-of-Create/Create/blob/mc1.21.1/dev/src/main/java/com/simibubi/create/content/kinetics/base/KineticBlockEntity.java), [rotation propagation](https://github.com/Creators-of-Create/Create/blob/mc1.21.1/dev/src/main/java/com/simibubi/create/content/kinetics/RotationPropagator.java)

## 2. Saint's Dragons

- **Entities and basic AI operations.** Register a real entity type with persistent state, attributes, dimensions, collision and spawn rules. Expose bounded queries, navigation, movement, targeting, riders and interaction hooks. Run mod AI in ordered entity ticks with declared world and entity reach. The engine owns movement, replication and saves.
- **Companion models and rider input.** Negotiate capabilities, deliver model and animation assets, and send entity animation and state updates. Use a versioned client-to-server message for rider input, checked against the sender and mount. Start with one dragon that spawns, flies, can be ridden and survives restart. Add items and biome or structure spawns through the shared registries and worldgen path as needed. [Saint's Dragons project](https://modrinth.com/mod/saints-dragons)

## 3. L_Ender's Cataclysm

- **Worldgen and encounters.** Register datapack-driven features, structure templates and placement in the right biome and dimension. Load saved arenas across restarts. Add spawn triggers and bounded arena queries and edits using the same bulk world interface.
- **Combat, items and models.** Extend the entity operations for phases, damage, targeting, projectiles and boss bars where the first arena requires them. Reuse the item registry for equipment and loot and the companion for boss models, attack cues and animations. Keep attack timing and phase decisions inside the mod. [L_Ender's Cataclysm project](https://www.curseforge.com/minecraft/mc-mods/lendercataclysm)

## 4. Legendary Monsters

- **Reuse the encounter path.** Register monster types and custom items, place them through features or structures, and reuse AI, combat, loot and companion animation operations. Port one encounter first. Add a host operation only when the encounter needs engine behavior the shared tables cannot express. [Legendary Monsters project](https://www.curseforge.com/minecraft/mc-mods/legendary-monsters)
