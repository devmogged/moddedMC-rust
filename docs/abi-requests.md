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

## Contraption block entity capture (resolved)

Host commit `457266d` supplies `World::block_entity` and `restore_block_entity`. The bearing stores the full saved data without its original coordinates and restores it after placing the rotated state.

## Moving-block writes (resolved)

Normal writes over a captured chest and furnace scattered two stacks on pickup and restored the same stacks on disassembly. Host commit `2fa8192` adds `UPDATE_MOVED`, which replaces the block without running the inventory scatter callback. The bearing storage scenario checks zero loose items and verifies the two stacks after a full restart and disassembly.

## Moving content block events (resolved)

Host commit `0aaef9b` exposes `MOVED_STATE_CHANGED` for replacements made with `UPDATE_MOVED`. The mod detaches moved blocks from the kinetic network without using the belt break path. The detach is covered by a unit test. The piston scenario verifies that its in-place `moving` transition keeps its own controller alive.

## Piston use and break hooks (resolved)

Host commit `0aaef9b` exposes `Entity::may_build` and `Interaction::on_player_will_destroy`. The sticky conversion scenario checks Adventure permission, the clicked face and Survival stack consumption. The break scenario checks that the callback can inspect the intact pole chain and remove the attached parts before the main break.

## Clockwork time

`World::day_time() -> Option<(i64, bool)>` returns the world's current Minecraft day time and the dimension's `natural` flag. Clockwork bearings use day time for both hands and multiply it by 24 in non-natural dimensions. The content tick's server-wide count does not reflect `/time set` or a paused daylight cycle.

## Elevator contact output

A content block needs to declare that it emits redstone and register a synchronous `signal(world, source, state, target, direct) -> u8` query. The host calls it for adjacent redstone queries and applies the existing neighbour updates when a content state changes. Create's elevator contact returns weak strength 15 while `powering=true`, except toward another elevator contact. Modded blocks also need to report which faces redstone wire can connect to. A contact excludes its front face.

## Super Glue and seat entities

`Entities::spawn_marker(world, position, key, nbt) -> Option<i32>` should create a loaded, persistent vanilla marker with the mod data already set. `Entity::discard() -> bool` removes the marker and its save. Super Glue stores a selection as an entity and takes it out of the world for each move. `Mobs::spawn` rejects marker entities at its living-mob check.

Seat support also needs `Entity::start_riding(vehicle, force) -> bool` and `Entity::stop_riding()` with the vanilla side effects, including the passenger-list packet. A seat vehicle needs a persistent, stationary non-living carrier with a riding offset and the ability to accept one passenger.

## Contraption actor hardness

`World::destroy_speed(pos) -> Option<f32>` returns the loaded block state's hardness with vanilla's negative value for unbreakable blocks. Moving drills read it both to reject unbreakable blocks and to calculate breaking progress while the contraption stalls.

Host commit `0aaef9b` provides `World::block_drops(pos, tool)` for actors to collect loot before it becomes item entities. Drill and harvester scenarios have yet to exercise it.

## Contraption entity queries

`Entities::entities_in` limits results to 256 before Create can exclude visual and non-collidable entities. A large contraption can fill that result with its block displays. Add a box query that filters by entity kinds and flags before applying the limit and exposes a continuation for larger results. The collision path also needs the entity's piston reaction, vehicle id, and no-physics status to apply Create's eligibility rules to all types.

Stock-player platform experiments need a player position update that sends the vanilla teleport and handles its acknowledgement. `Entity::set_position` only updates the server entity. The shulker experiment needs a no-AI setter for the spawned shulkers so their vanilla behavior does not change the result.
