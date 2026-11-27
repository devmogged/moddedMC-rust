# Compass group and locate-offset integration contract

## Reference behavior

Explorer's Compass 3.3.2:

- `StructureUtils.getGroupForStructure`: scan structure-set registry in
  its iteration order and return the first set containing the chosen structure.
- `getStructuresForGroup`: return the set's declared weighted entries in
  order, not a structure tag, a name-prefix group, or a list sorted by weight.
- `SearchWorkerManager.createWorkers`: for each member, enumerate all its
  active-generator placements in their order. Merge by placement identity
  into an insertion-ordered map. Start its first worker; only after it
  exhausts, start the next. A success cancels remaining workers.
- `StructureSearchWorker.getStructureGeneratingAt`: query presence of
  each allowed member at the candidate, not arbitrary generation of the
  requested member. A weighted set generates only its first valid candidate.
- `ConcentricRingsSearchWorker`: within a single placement choose the
  closest valid candidate by chunk-center distance; stable ties keep the
  first original ring entry. Random spread uses its perimeter traversal,
  returning the first valid hit. The manager does not compare results across
  different placement workers.
- `ExplorersCompassItem.succeed`: stores actual found structure ID, group
  flag, position and previous positions. `searchForNextStructure` resolves
  the first group of that actual ID again. A new explicit group/individual
  selection starts fresh history. Terminal failure clears previous history
  and the group flag. On find-next, the worker's failure ID is the previous
  actual member, even though the active stack identifies its group. The
  original GUI only enables find-next in the found state when the actual
  found structure is selectable. A group can discover a hidden member,
  but its next action cannot be exposed through a hidden individual entry.

The mod implementation preserves this separation: group IDs and actual
structure IDs are separate UI actions, and the group worker returns the
actual found member. The stock-client menu exposes a separate group page so
individual IDs and existing pagination remain unchanged.

## Minimal SDK/host surface

Separate optional interface `WorldgenStructureGroupsV1` (do not extend an
existing table without size/version handling):

```rust
groups: unsafe extern "C" fn(WorldRef, Sink) -> bool
```

One sink record per set: key = registry ordinal, value = UTF-8
`group_id\0member_id\0member_id...`. No empty tokens. Use canonical
namespace-qualified identifiers and preserve registry/member order,
including sets disabled in this dimension (first-group membership is
registry-wide). The safe SDK wrapper validates UTF-8, consecutive ordinals
and nonempty tokens, returning `Option<Vec<StructureGroup>>`. Group
planning calls the existing `placements(world, member)` separately for
each member, so eligibility still follows the active generator.

Existing `structure_start(world, placement, target, x, z)` is sufficient
for the initial implementation because it resolves the weighted winner
before comparing the target. The mod checks one member per tick/advance;
it resumes that member on Pending and moves to the next only on Absent.
Never report a member if it loses the weighted selection. A future
optimization can return the actual winner in one call with a string sink,
without changing mod selection semantics.

The mod uses the optional SDK wrapper `StructureGroups`, with
`require_groups()` and `groups(self, world: World<'_>)`. It can still run
individual-only behavior on a host missing that interface; group menus
appear only after group registry retrieval succeeds.

## Locate offsets

Minecraft 26.3 `AbstractSpreadingStructurePlacement.placementCodec`:
`locate_offset` is an optional Vec3i defaulting to zero, each axis limited to
±16. `StructurePlacement.getLocatePos` returns
`BlockPos(chunkMinX, 0, chunkMinZ).offset(locateOffset())`.
Vanilla buried treasure already declares [9,0,9], so dropping this field
misplaces vanilla as well as custom searches.

Required host/engine changes:

1. Parse the optional [i32; 3] field in pumpkin-worldgen-model's
   StructurePlacementStruct; validate each coordinate against ±16.
   Preserve it in both codegen emission and runtime datapack build.
2. Add a default-zero locate offset field to generated StructurePlacement
   and all constructors/test fixtures. Do not add it to RingKey: it
   changes the returned locate position, not the ring layout.
3. Centralize chunk-to-locate conversion (checked chunk coordinate plus
   offset) for host worldgen_lookup.structure_start and engine
   structure_finder's locate paths. The latter currently constructs
   unconditionally zero-offset BlockPos in its actual-start, concentric,
   and placement-only random spread paths. For compass worker search
   order/radius, continue comparing chunk centers before the offset
   (upstream workers do that); returned tracker/history positions are
   offset, including Y.
4. Existing mod stack codec MCc1 remains readable. MCc2 adds an Explorer
   group bit and compact signed-byte Y for every history entry; all
   101 default-history results fit the 1024-byte cap for normal IDs.
   Before starting, reserve the extended encoding including possible
   member ID length so a successful job cannot fail to save its result.
   Do not infer Y from X/Z or discard it on restart.

The weighted-group fixture tests two nonvanilla ring positions and
weighted actual members, nonzero offsets and persisted find-next.
The bot's wire tracker assertion now accepts an optional Y argument.
The weighted-group and individual-offset scenarios assert Y=7 as well
as X/Z and dimension, including after restart. The group scenario also
checks the previous actual member on exhaustion, no find-next action after
failure, and a fresh explicit group search with cleared history.

## Outstanding parity outside this change

- Visibility/blacklist configuration and
  `c:hidden_from_locator_selection` now filter direct choices;
  full group membership stays registry-defined. See CONFIG.md. Current menu restricts
  direct entries to the active dimension, whereas Java lists other
  dimensions and presents generating-dimension metadata.
- The Java worker first checks saved structure presence, then generates
  starts if unknown. Pure generation in the host agrees with fresh seed
  tests but does not yet account for existing imported/modified chunk
  starts.
- Configurable search limits and coordinate visibility now work in source.
  XP costs, durability and teleport still need host capabilities.
- Focused reset, visibility and multiplayer protocol cases passed on
  2026-11-27; see the fixture document for exact logs, remaining cases
  and the interrupted-search fixture correction. No separate build or
  scenario may start while the prime build gate is held.
