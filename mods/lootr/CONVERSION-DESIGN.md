# Native conversion: integration proposal

This is a design, not a claim that generated-container conversion is already
working. Reviewed against Pumpkin submodule `c1ab1872727c72a600cf96c7debd89ceb1205c72`
and the local Lootr 1.25.42.124 (26.3) Java reference. All Pumpkin paths below
are relative to `engine/pumpkin/crates`. No shared source has been edited.

## Existing generation and load path

* `pumpkin-world/src/generation/structure/structures/desert_pyramid.rs:239-243`,
  `buried_treasure.rs:136-140`, `igloo.rs:222-223`, and
  `structures/mod.rs:644-645,681-682` write a vanilla pending `LootTable`
  and `LootTableSeed`. Feature `features/monster_room.rs:153-154` does the
  same for dungeons. Template pieces assign absent seeds at
  `structure/template/template_piece.rs:258-268`. Do not patch individual
  generators, generate chunks from a mod callback, or substitute a table.
* `pumpkin-core/src/world/mod.rs:565-615`: active loaded chunk transitions
  and pending migration queue are processed, with migrations deferred until
  generation postprocessing is finished. Locks on active chunks/tracker are
  dropped before postprocessing and migration. This is the eager conversion
  scheduling boundary.
* `world/mod.rs:7603-7625`: `migrate_pending_block_entities` copies pending
  positions out of a chunk lock before materializing them. Process loot
  candidates here before normal migration or immediately after materialization
  with a fresh live entity lookup; include already-materialized entries.
  A subtlety: `update_block_entity` at `world/mod.rs:7627-7648` writes
  client-oriented `chunk_data_nbt` into pending NBT, and the chest macro at
  `block/entities/chest_like_block_entity.rs:113-126` omits its pending
  loot key and seed from that payload. After migration, the live entity's
  full `write_internal` is therefore the authoritative source, not the
  pending copy. The full snapshot at `world/mod.rs:851-884` restores
  authoritative NBT during ordinary save/unload; normal unload at
  `:5821-5836` saves before removing live entities. A deferred candidate
  must not mistake the partial client NBT for an already-unpacked chest.
* `world/mod.rs:7381-7435`: lazy `get_block_entity` first checks the live
  cache, then copies pending chunk NBT, materializes, and inserts it.
  Do not execute conversion callbacks here: nested getters can recursively
  materialize/convert and expose a stale Arc. This function may enqueue a
  position for the safe tick boundary instead.
* `world/mod.rs:7226-7259`: command NBT application replaces the live
  entity and writes full NBT; queue a candidate **after** this completes.
  `world/mod.rs:7496-7548` also handles entity installation and
  `block/entities/chest_like_block_entity.rs:173-188` applies item
  components (which can introduce pending loot). A candidate must be queued
  after assignment, not merely on initial vanilla chest placement.
* `world/mod.rs:6867-6877` is the no-load state getter. If absent, skip;
  the next active chunk load should rediscover the pending NBT. Upstream
  `BlockEntityTicker.Entry.getChunkLoadStatus` also waits for the
  surrounding 5-by-5 chunk neighbourhood, with world-border exceptions.
  Incomplete neighbourhoods must defer and retry boundedly, never force-load.

## Pre-unpack guard: mandatory even with eager conversion

* `pumpkin-core/src/block/blocks/chests.rs:204-239` is the central chest
  unpack path. Existing plugin cancellation is unsuitable: it intentionally
  consumes the table and leaves an empty inventory (lines 205-228).
* `world/loot.rs:538-555` atomically takes the key under its mutex, releases
  the lock to roll, then fills only empty slots. A conversion guard must run
  **before** this irreversible take and before LootGenerateEvent.
* The three direct callers are `chests.rs:271-290` (automation inventory),
  `:294-323` (comparator), and `:330-370` (menu factory). An early return
  from the unpack function alone is unsafe: those callers still use their
  original Arc. Explicit outcome must propagate: a converted or deferred
  container is unavailable to automation/comparators and must never open
  a vanilla inventory.
* `block/entities/hopper.rs:354-379` uses `chest_container` for chest
  blocks, so this covers its deferred-loot access. A generic direct
  `get_inventory` access still needs audit for commands, other mods and
  inventory scatter. `block/entities/mod.rs:173-177` scatters items on
  replacement. `command/commands/item.rs:65` gets the raw inventory;
  player/admin insertion into pending loot is possible and must not be lost.
* `chests.rs:373-398` normal use should check conversion before factory
  construction. On successful conversion, redispatch the actual new block
  behavior once (no recursive vanilla loop), or consume and require a new
  use. Never use the old factory after replacement. A deferred eligible
  container consumes the click without claiming or generating loot.
* `block/entities/barrel.rs:25-72` currently ignores LootTable and seed,
  loading only items. `block/entities/mod.rs:82-98` explicitly says
  chest-like deferred unpack is not implemented for barrels and other
  vanilla containers. Barrel support needs pre-materialization NBT
  conversion or the engine must retain their loot fields. It is not enough
  to add a Lootr barrel registration.

## Minimal generic ABI

Add a separate versioned `moddedmc:container_conversion` interface rather
than expanding a Lootr-specific host. A synchronous read-only callback
receives a loaded `World`, position, exact source state ID, pending table
ID, seed, and source NBT/component metadata (borrowed, bounded). It returns:

* Pass: mod has no claim on this candidate; normal vanilla behavior.
* Defer: eligible, but not safely ready; keep table and block intact.
* Convert: return destination content state ID and initial named-root NBT.

Register callback once with mod ownership, deterministic priority/order,
and require destination state ownership. Only a successfully registered mod
can claim conversion. A callback is a proposal, not authority to mutate the
source; the host performs the guarded replacement. No callback holds a
chunk map, loot mutex, item mutex, world tracker or entity-map guard.
The returned NBT must be parsed/size-checked and destination entity
constructed before any source mutation. Mod may decline unsupported
metadata, lock, double chest or non-empty source.

Engine-facing hook outcomes are distinct from the mod callback result:
`Pass`, `Deferred`, `Converted`. A stale candidate or failed compare
is not success; re-read or defer without rolling. An implementation can
use existing OnceLock hook style (`world/mod.rs:323`) and a bounded,
deduplicated per-world queue drained at active chunk migration/tick.

## Safe replacement protocol

1. Read current loaded source state, live entity Arc identity (or pending
   NBT identity before materialization), table/seed, and metadata without
   unpacking. Resolve a destination matching the source facing and water
   logging (Lootr chests are single).
2. Ask the mod to propose replacement; validate state ownership,
   NBT and supported source fields. A lock or pre-existing items should
   decline conversion until the port preserves them.
3. Acquire a short engine-side reservation covering the source entity
   identity and pending loot. The existing key-only mutex is not sufficient
   as simply taking the key permits another caller to observe
   `has_loot_table() == false` and open a shared empty chest. Reserve
   explicitly (e.g. pending/claimed/converting state) and have vanilla
   readers treat converting as deferred. Coordinate the reservation with
   NBT reload, block replacement, and inventory updates, including
   concurrent inserts; an arbitrary thread may hold the old Arc.
4. Re-check live entity identity, state, original table/seed, and empty
   items under the reservation. A state-only compare is insufficient:
   `world/mod.rs:5915-5954` CAS protects state, not a same-state block
   entity replacement. Commit replacement state + new entity + full
   chunk NBT together before exposing it or issuing notifications.
   This needs a core transaction boundary, not two SDK world writes.
5. Suppress old block-entity replacement side effects
   (`world/mod.rs:6004-6017`; existing
   `SKIP_BLOCK_ENTITY_REPLACED_CALLBACK` flag). Do not emit item drops,
   generate loot or expose an intermediate uninitialized mod chest.
   Successful replacement persists the table and seed in the new entity
   and assigns a new persistent Lootr ID. Failure restores reservation
   to the original pending table and leaves state, metadata and items
   untouched. Do not call arbitrary callbacks while holding the guard;
   release before normal client/neighbour notifications.

Initial supported conversion can be restricted to unloaded-to-loaded
single `minecraft:chest` candidates with no unsupported metadata or
contents; eager conversion before publishing a live vanilla entity is
simpler than a general live swap. It still needs a pre-unpack guard for
lazy accesses and NBT assignment. For pending NBT, state and pending
NBT must be published atomically with respect to lazy materialization.
Do not claim parity for connected halves, custom tags, copper/trapped
chests or barrels until corresponding mod registrations and metadata
semantics exist.

## Integration tests

Use the existing fixture table with 27 nonstackable swords. First test
explicit `/setblock minecraft:chest{LootTable:...,LootTableSeed:...}`
and `/data merge`, then a naturally generated pyramid chest at the
known seed/position used in the compass work. Assert the custom block
state appears before any opening, the original table/seed survives
conversion and normal restart, and independent players both receive
the loot. Exercise hopper/comparator access before player open,
same-position replacement, chunk unload/reload, failed table resolution,
and unrelated player-placed or already-unpacked vanilla chests remaining
vanilla. Force a failure/competing update and assert no item drop or
lost pending table. A synthetic explicit-table test alone is not proof
of generated chunk conversion.

## Further source fidelity

Upstream conversion intercepts `tryLoadLootTable`, `setLootTable`,
and `unpackLootTable` in `MixinRandomizableContainer`; it queues candidates
and cancels unpack without clearing the pending table. `BlockEntityTicker`
filters conversion tags, dimension/structure/table lists, world border and
loaded neighbours before it calls `replaceEntity`. That method clears
the original table before block replacement and copies metadata and
components. These are functional requirements, not implementation details
to copy verbatim.
