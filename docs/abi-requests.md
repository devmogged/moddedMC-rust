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

Host commit `0aaef9b` exposes `MOVED_STATE_CHANGED` for replacements made with `UPDATE_MOVED`. The detach unit test covers moves without chain destruction. The moving belt scenario checks the whole chain and item count across pickup, restart, and disassembly. The piston scenario verifies that its in-place `moving` transition keeps its own controller alive.

## Piston use and break hooks (resolved)

Host commit `0aaef9b` exposes `Entity::may_build` and `Interaction::on_player_will_destroy`. The sticky conversion scenario checks Adventure permission, the clicked face and Survival stack consumption. The break scenario checks that the callback can inspect the intact pole chain and remove the attached parts before the main break.

## Clockwork time (resolved)

`World::day_time() -> Option<(i64, bool)>` returns the world's current Minecraft day time and the dimension's `natural` flag. Clockwork bearings use day time for both hands and multiply it by 24 in non-natural dimensions. The content tick's server-wide count does not reflect `/time set` or a paused daylight cycle.

Host commit `a553b83`: `World::day_time()` returns `DayTime { ticks, fixed_time }`. 26.3 has no `natural` flag and keeps day time on world clocks, so `ticks` is the overworld clock (`getOverworldClockTime()`, the value every dimension shared in 1.21.1) and `fixed_time` is `hasFixedTime()`, which holds exactly for the dimensions 1.21.1 called not natural. Multiply by 24 when `fixed_time`. Require `WorldExtension::Survival`.

Repro on engine `24dbedcdc` with the local clockwork bearing slice. In a fresh overworld, run `gamerule minecraft:advance_time false`, `time set 21250`, and `time query daytime`. The query prints `The time is 21250`. Power an upward clockwork bearing at 15 RPM for five seconds. Reading `Bearing.Angle` and `Bearing.Clockwork.MinuteAngle` with `data get block` gives `-180f` and `0f`. At 03:15 both should be `-90f`. The observed angles are the targets at tick zero. Both hands still assemble, survive a restart and return to their original blocks when power stops.

Host commit `1ed72ae` reads the live overworld clock, including for callers in fixed-time dimensions. The clockwork scenario reads `-90f` for both hands at 03:15 and checks the minute hand after restart.

## Elevator contact output (resolved)

A content block needs to declare that it emits redstone and register a synchronous `signal(world, source, state, target, direct) -> u8` query. The host calls it for adjacent redstone queries and applies the existing neighbour updates when a content state changes. Create's elevator contact returns weak strength 15 while `powering=true`, except toward another elevator contact. Modded blocks also need to report which faces redstone wire can connect to. A contact excludes its front face.

Host commit `1ca3f7f`: `Interaction::on_redstone(reg, signal, connects)`. `connects(state, direction)` is `isSignalSource() && canConnectRedstone(direction)` (exclude the front face there); `signal` is `getSignal`, or `getDirectSignal` when `direct`, and the host calls it for every neighbour query as vanilla does, whatever `connects` says. Block writes already notify neighbours. Require `InteractionExtension::Redstone`.

## Super Glue and seat entities (resolved)

`Entities::spawn_marker(world, position, key, nbt) -> Option<i32>` should create a loaded, persistent vanilla marker with the mod data already set. `Entity::discard() -> bool` removes the marker and its save. Super Glue stores a selection as an entity and takes it out of the world for each move. `Mobs::spawn` rejects marker entities at its living-mob check.

Seat support also needs `Entity::start_riding(vehicle, force) -> bool` and `Entity::stop_riding()` with the vanilla side effects, including the passenger-list packet. A seat vehicle needs a persistent, stationary non-living carrier with a riding offset and the ability to accept one passenger.

Host commit `518bfa8`: `Entities::spawn_carrier(world, kind, position, key, nbt)` with `CARRIER_MARKER` (a saved vanilla marker, never sent to clients, mod data set before it is added) and `CARRIER_SEAT` (an empty, invisible, saved `item_display` whose passenger seat is its own position, so place it where the rider's vehicle attachment point belongs). `Entity::discard`, `start_riding(vehicle, force)`, `stop_riding` and `vehicle`. Require `EntitiesExtension::Carriers`. Pumpkin has no boarding cooldown, so that part of `canRide` is absent.

## Contraption actor hardness (resolved)

`World::destroy_speed(pos) -> Option<f32>` returns the loaded block state's hardness with vanilla's negative value for unbreakable blocks. Moving drills read it both to reject unbreakable blocks and to calculate breaking progress while the contraption stalls.

Host commit `0aaef9b` provides `World::block_drops(pos, tool)` for actors to collect loot before it becomes item entities. Drill and harvester scenarios have yet to exercise it.

Host commit `a553b83`: `World::destroy_speed(pos)` returns the hardness, negative for unbreakable. Require `WorldExtension::Survival`.

## Harvester replant checks (resolved)

`World::can_survive(pos: Pos, state: u32) -> Option<bool>` runs the candidate block state's vanilla `canSurvive` against the loaded world at that position. It makes no world changes and returns `None` when the state or required cells are unavailable. Create checks the reset crop after rolling its loot. In 26.3, `CropBlock.canSurvive` checks both the support and raw light of at least 8. `Content::block_item(state: u32) -> Option<String>` returns the state's `Block.asItem()` id. Create uses that item to identify which drop to subtract when replanting.

Host commit `a553b83`: `World::can_survive(pos, state)` (Pumpkin's `can_place_at` with a world, which is `canSurvive` including the crop light check) and `Content::block_item(state)` (`Block.asItem()`, `minecraft:air` for none; carrots give `minecraft:carrot`, wheat `minecraft:wheat_seeds`). Require `WorldExtension::Survival`.

## Contraption entity queries (resolved)

`Entities::entities_in` limits results to 256 before Create can exclude visual and non-collidable entities. A large contraption can fill that result with its block displays. Add a box query that filters by entity kinds and flags before applying the limit and exposes a continuation for larger results. The collision path also needs the entity's piston reaction, vehicle id, and no-physics status to apply Create's eligibility rules to all types.

Stock-player platform experiments need a player position update that sends the vanilla teleport and handles its acknowledgement. `Entity::set_position` only updates the server entity. The shulker experiment needs a no-AI setter for the spawned shulkers so their vanilla behavior does not change the result.

Host commit `518bfa8`: `Entities::entities_matching(world, min, max, except, exclude)` filters by flag bits before counting and pages internally past 256, so it returns every match. New flags `ENTITY_NO_PHYSICS` and `ENTITY_PASSENGER`, plus `Entity::vehicle()`. Piston reaction stays `ENTITY_PUSH_IGNORED`; say if you need the other reactions. Require `EntitiesExtension::Carriers`.

## Block breaker effects and drop rule

Moving Create drills use `World::block_drops` before removal so the actor can own the output. That call rolls loot even when block drops are disabled. Expose `World::block_drops_enabled() -> Option<bool>` for the live block-drops game rule so actors can skip both the roll and spawning when it is off.

For drill progress, expose `World::destroy_block_progress(breaker_id: i32, pos: Pos, stage: i8) -> bool` with vanilla stages 0 through 9 and -1 to clear. For entity contact, expose `Entity::hurt(damage_type: &str, amount: f32) -> bool` with vanilla hurt cooldowns and the registered damage type. The drill uses `create:drill`.
