# Create ABI requests

## Comparator output (deferred)

Create's Depot supplies an analog signal derived from its held stack and eight output slots. The content registration also needs a per-block analog-output flag. Pumpkin reads the static `HAS_ANALOG_OUTPUT_SIGNAL` state flag before calling the block, and content currently inherits that flag from its vanilla physical template. A generic `moddedmc:block_world` extension can expose `set_analog_output(world: WorldRef, position: Pos, strength: u8) -> bool`. A successful call marks the loaded content block as an analog source and records a value in the range 0 through 15. The host notifies adjacent comparators when the value changes and invalidates the value on removal or unload. The mod recomputes it from its persisted inventory after loading the block.

## Vanilla cooking resources (resolved)

Host commit `0538639` exposes the vanilla cooking recipes with datapack overrides taking precedence. `create-fan.txt` passed on a fresh world with default datapacks. The saved loose-item timer had 71 ticks left before saving and 11 after reconnecting, then the bot picked up the haunted result.

## Effective stack components (resolved)

Host commit `36058bc` exposes the effective value through `ItemStacks::component`. The fan scenario verifies that lava airflow blasts raw iron and leaves a netherite ingot intact.

## Block and fluid state identity (resolved)

Host commit `44e07b8` exposes full selectors and fluid ids. The fan scenario washes ice through a waterlogged oak fence using both queries.

## Crushing collision context (deferred)

Create's controller keeps its normal 13/16 collision for other entities and returns an empty collision shape for its captured entity and its ejected items. The existing shape query reads the standard shape. A content collision callback taking `(world: WorldRef, pos: Pos, state: u32, entity: EntityRef) -> bool` could let a mod opt a particular entity out of the state's collision. A null entity keeps the standard shape. The callback must answer synchronously from current state and avoid changing the world.

## Contraption block entity capture

P1d needs the complete saved NBT of vanilla chests, furnaces and other movable block entities. Append `read_block_entity(world: WorldRef, pos: Pos, out: Sink) -> bool` and `restore_block_entity(world: WorldRef, pos: Pos, nbt: Str) -> bool` to block-world. Capture includes the vanilla type id and components. Restore runs after the destination block is placed, checks that the saved id is valid for the destination state, and uses the destination coordinates instead of the old `x`, `y` and `z` fields. It replaces the live entity, marks the chunk dirty and performs the same client update as a vanilla structure placement. Both calls must refuse unloaded positions. The current `World::entity` and `set_entity` only accept content block entities.
