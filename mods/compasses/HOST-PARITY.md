# Remaining generic capability contracts

Source baselines:

- [Nature's Compass 26.3-3.3.1](https://github.com/MattCzyr/NaturesCompass/tree/2202890ba7eec2312e6ea5c7734b2847e6b3a1c7/src/main/java/com/chaosthedude/naturescompass)
- [Explorer's Compass 26.3-3.3.2](https://github.com/MattCzyr/ExplorersCompass/tree/3dcbd585fc100719fd6aac078941a818047395cc/src/main/java/com/chaosthedude/explorerscompass)
- [Stock 26.3 compass item definition](https://github.com/misode/mcmeta/blob/26.3-assets/assets/minecraft/items/compass.json)

These contracts distinguish source implementation from verified behavior.
The Compass mod currently uses only the published interfaces.

## Needle

Both upstream AngleState classes fall back to the level respawn position
outside the found state. They reconstruct a found target in the current
dimension using saved X/Z, which can point to unrelated coordinates after
a dimension transfer. The native adaptation intentionally stores the
actual generating dimension and preserves it on transfer. It must not
invent a successful lookup in another world.

The vanilla 26.3 item model selects between two compass range dispatches
by the presence of the lodestone tracker component. The frame thresholds
in the host's existing `compass_item_definition` already match those in
both vanilla branches. The shared host now implements the following
generic behavior in source; compilation and runtime verification are pending:

1. Wrap the existing frame dispatch in a `minecraft:condition` with
   property `minecraft:has_component` and component
   `minecraft:lodestone_tracker`. Use a lodestone dispatch when true
   and a spawn dispatch when false.
2. Atomically remove the lodestone tracker data component when the existing
   held-item or player-stack compare-and-replace operation receives
   `target=None`. An empty target *inside* a present component is not
   equivalent: vanilla's predicate still selects the lodestone branch.
   The host's new `TrackerUpdate::Clear` removes the component in both
   held-item and inventory-CAS paths, with no extra ABI field.
3. Preserve the registered item-model identity on the wire. Its reverse
   mapping is how the client stack is reconstructed as the custom item.
   Do not select a different item model by mutating the server stack.

The mod's MCc1/MCc2 codec projects a tracker only in Found state, using the
saved dimension and exact locate position. It rejects coordinates outside
the host's horizontal bounds or the signed 12-bit wire Y field. Old or
malformed state is repaired on use even when the current catalog is empty.

Acceptance should check both component presence and its exact target,
including reset, active search, failure, successful search, hand swap,
restart, and cross-dimension transfer. Existing `expect tracker none`
checks only absence of a target, not absence of the component. Real
stock-client needle animation remains a separate visual verification.
The shared bot now provides `expect tracker SLOT absent`, which requires
the entire component to be missing and rejects a present tracker with
an empty target. Negative, same-item and extended interrupted fixtures
passed this strict assertion on idle/reset/searching/restarted stacks
on the coordinated optimized host and 221,308-byte Compass package.
The host model-definition unit test checks the conditional property,
component name, two target kinds and equality of all 33 frame entries.
It does not substitute for checking the resource pack actually delivered
to a stock client, or the actual presence of the component on the wire.

## Dimension catalog

Source `BiomeUtils.getAllowedBiomeIds` and
`StructureUtils.getAllowedStructureIds` enumerate global registries and
filter hidden/blacklisted identifiers. Their generating-dimension helpers
then inspect the biome source of every server level. Structure eligibility
for the original metadata is the intersection of the structure's biome
holder set with a world's possible biome set, even when there is no
locatable placement. The actual search uses the active generator's
placement list, not that metadata as a substitute for generation.

The current SDK exposes loaded dimension names and a world handle in a
specific callback, plus world-specific biome/structure lists. The latter
is restricted to structures with enabled locatable placements. It cannot
enumerate full registry entries or traverse all worlds from a player
callback. An empty list does not establish that every registry entry is
absent from the server.

A generic synchronous catalog interface should return one coherent
registry snapshot, with the identifier and all generating dimensions per
entry (including a known-empty set). Provide all registered entries,
including those not currently generated in any dimension. An unavailable
snapshot must be distinguishable from an empty successful snapshot.
Preserve canonical identifier strings and expose the existing
per-dimension locator separately. Do not cache previous worlds' partial
lists as authoritative after datapack reload or world unload.

The mod will apply its config and hidden tags to that global catalog,
display generating dimensions, and still authorize search and find-next
against the player's actual current world and exact stack state.

## Search costs and repair

The source General configs specify default and per-target XP levels (0–3),
and optional durability (0–10,000 successful searches). Both item classes
charge XP on search start, including retry and unsuccessful search; the
group start uses the group ID, while find-next uses the previous actual
member ID. Infinite-material players are exempt. Damage increments only
on success, not cancellation or failure, and a broken compass remains an
item to repair. Sneak reset clears search state but not damage.

Implement XP charging as a transaction with exact-stack CAS acceptance
and sufficient-level validation. Implement successful-search damage
atomically with the resulting item state and tracker. An ordinary tool
break operation that deletes the item is not source-equivalent. Both
upstream repair recipes are ordinary shapeless recipes: one mod compass
plus one vanilla compass produces a fresh compass of the same mod type.
The native resources already ship these recipes. A fresh result must not
copy the input's custom data, damage, history or lodestone tracker.
There is no upstream requirement for a damage-gated repair recipe.
The missing generic capability is atomic successful-search damage, not
a special crafting hook. The existing stock-grid repair path still needs
protocol verification.
The current default (zero cost and no durability) is implemented;
explicit enabled settings reject startup with a precise error.

## Teleport

Source `PlayerUtils.canTeleport` checks operator status or singleplayer
cheats, plus the Nature's Compass permission node. The config description
also mentions creative, but the checked implementation does not grant
teleport merely for creative status. `TeleportPacket` rechecks permission
server-side and requires a found stack. It scans up/down from sea level
for a two-block free space above a non-free block.

The native port needs a permission query and a safe destination search
with loaded/generated terrain. Use the exact held-stack identity and
saved target dimension. Reject stale or unprivileged requests; do not
expose the unconditional Player::teleport call as a public menu action.
The source's fixed fallback Y=256 is not a safe default for arbitrary
dimensions and should not be copied as if it were a verified landing.
