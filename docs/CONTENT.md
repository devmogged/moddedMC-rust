# Content model (design for v1)

This is how mods add blocks, items, entities and the rest so that Create, Saints Dragons and Cataclysm can be ported faithfully. The host supports first-class blocks, items and block entities alongside the legacy M1/M2 stand-in API. Engine work lands in `devmogged/Pumpkin`. All [ports](PORTS.md) get access through named, versioned interfaces.

## Principles

1. **Modded content is first-class in the engine.** A Create shaft is a real block with real block states in the chunk palette, not a barrel with a tag. Ticking, collision, light, redstone, drops and saving all go through the same code as vanilla blocks.
2. **One server truth, two client views.** The server keeps the real ids. Each connection gets a view of it:
   - **Vanilla view** (stock client, no companion): every modded block state, item and entity is mapped to something vanilla can show. A block state becomes an unused vanilla state retextured by the pack's resource pack (note block, mushroom block and tripwire states and the like, the way Polymer does it), or a carrier block plus display entities for complex models. Items get custom item models. Entities become display or vanilla carrier entities.
   - **Companion view**: the companion registers a fixed pool of generic blocks, items and entity types at client start. At join, the server sends definitions (model, shape, collision, light, sounds) for the pool slots this pack uses, and then sends modded ids directly. Nothing about the pack is compiled into the companion.
3. **Ids are append-only and saved per world.** Modded registry entries get ids after vanilla's, and the world stores the namespaced-id-to-number map (`moddedmc/registry.dat`). Adding a mod appends; removing one leaves a hole that loading code clears (Forge behavior). The same applies to the network: each connection gets its own id table at configuration time.
4. **Behavior is Rust, data is data.** Block properties, shapes, recipes, loot tables, tags, sounds and models are data, ideally the mod's own JSON from its source or jar, loaded through the same paths as vanilla datapacks. Behavior (kinetics, contraption assembly, AI) is Rust code behind traits shaped like vanilla's (`BlockBehaviour`, `ItemBehaviour`, entity tick, `Goal`).
5. **Keep the original mod's names.** Ids, NBT field names, units and block state property names match the original mod. The harness can then probe the same fields on both sides, and Java worlds can later be imported.

## Registries in v1

Blocks and block states, items, block entities, entity types, menus (container GUIs), recipe types and serializers, loot functions and conditions, sounds, particles, creative tabs, commands and data components. Worldgen (biomes, features, structures) comes through datapacks; see E2 in the backlog.

## Engine work this needs (devmogged/Pumpkin)

- A dynamic block-state registry after the generated vanilla one, with state ids above vanilla's maximum. Today `BlockStateId` is a `u16` and vanilla 26.3 uses 35,723 states, which leaves about 29,800 for mods. That's enough for the first packs, but a big pack will need `u32`. Registration happens at startup before any world loads. Definitions are leaked to `'static` and frozen, so `&'static Block` and `&'static BlockState` keep working everywhere: an id below the vanilla count reads the generated array, and anything above it reads the frozen modded table.
- Per-connection id translation on every outgoing packet that carries block states, items or entity types (chunks, block updates, inventories, entity spawns and metadata), plus incoming translation for item use and clicks.
- Resource pack generation and delivery, with hashes, for the vanilla view.
- A companion handshake (custom payload channel) that switches a connection to the companion view.
- The trait hooks that vanilla's block, item and entity code already uses, exposed to mods as stable C tables with callbacks for the relevant engine operations.

## Order

1. Dynamic block registry, per-world id map, vanilla-view translation, generated resource pack. The test is a Create shaft rendered from a retextured vanilla state on a stock client.
2. Items and block entities on the same path.
3. Entities, display-entity rigs and animated models (GeckoLib format, for Saints Dragons and Cataclysm).
4. Companion view.


## Java vanilla view

The host freezes custom definitions before the root world opens and stores the append-only raw id map under `world/moddedmc/registry.dat` (using the configured level folder name). Unregistered saved state ids resolve to air. Anvil chunks save names and properties. Pumpkin chunks use the persisted numeric ids.

The current Java connections all use the same vanilla view. The host reserves powered note block states for static block models or invisible carriers with tracked item displays. As those slots fill, it uses powered target states and intermediate honey levels in bee nests and beehives. Opaque full-block models without a mod-specific block entity can also use dry leaf states with distance 1 through 6. Ordinary dry leaves sent in a reserved state keep their species and persistence, and use distance 7 on the wire. Wet leaf states keep their vanilla mapping. A vanilla note block that occupies a reserved slot is sent as the unpowered variant with the same note and instrument. A vanilla target with a reserved power value is drawn as an idle target. Bee nests and beehives with a reserved level are drawn with the same facing and the empty model. The server keeps their actual power or honey level. Custom items use a paper base plus an item-model component. The server keeps the actual item in inventory and uses its own state when processing clicks and item use. An incoming creative stack with a registered model and the matching carrier is restored to the actual item. Chunk palettes and block updates translate at their wire boundaries.

The [companion handshake](COMPANION.md) currently negotiates hashed asset delivery. It loads the same resource definitions from its cache while these vanilla-view mappings remain in use.

Pack generation copies the referenced model and texture dependencies from local jars. The generated ZIP is required when real content is present and takes the Java pack offer. The stock outline follows the carrier. Server-side collision and outline queries use the registered shapes.
