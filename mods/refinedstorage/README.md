# Refined Storage — native prototype (2.0.9 baseline)

This Rust mod is based on **Refined Storage 2.0.9** by Refined Mods, official
MIT-licensed source
[`refinedmods/refinedstorage2` at `v2.0.9`](https://github.com/refinedmods/refinedstorage2/tree/v2.0.9),
commit `0dec63affe8147702b020843c22748483ef18c01`. The original license is in
[`LICENSE.md`](LICENSE.md). Content identifiers and the 43 progression JSON
recipes are copied from `refinedstorage-common/src/main/resources/data/refinedstorage/recipe/`
at that exact revision. Fourteen additive `c:` item tags plus a
`refinedstorage:grids` tag for supported variants make the commonly used
ore, glass, redstone, silicon, quartz and ender-pearl ingredients available without another
platform's Forge item tags. The custom block states use stock Java 26.3 vanilla
carriers and models; no original texture binaries are distributed.

**BUJA source alignment:** the pinned 65-JAR inventory contains
`refinedstorage-1.12.4.jar` (Forge, Minecraft 1.20.1), from the separate
`refinedmods/refinedstorage` v1.x project. The existing disk/grid/importer and
autocrafting prototype still follows v2.0.9; this update adds a separate,
source-pinned v1.12.4 Crafter, Crafter Manager, and processing-pattern subset
from commit `8ed764669911a5e40c15075bb430d10e4eec483d`. The result is a hybrid
native implementation, not yet a complete source-faithful port of either
version. See the boundaries below before relying on processing automation.

## Implemented gameplay

The registered blocks include `controller`, `cable`, `disk_drive`,
`grid`, `crafting_grid`, `pattern_grid`, `autocrafter`, `external_storage`,
`crafter`, `crafter_manager`, `importer`, `exporter`, `wireless_transmitter`, and four
`1k`/`4k`/`16k`/`64k` Storage Blocks. Network
discovery uses the live loaded-world graph, caps nodes at 4096, rejects multiple
controllers, and never silently connects across unloaded chunks. All connected
digital Disk Drives appear in an accessible stock-client 54-slot Grid menu;
external inventory connections are also listed with component-specific identity.

The **Disk Drive has eight slots**, with source capacities of **1,024 / 4,096 /
16,384 / 65,536 items** per `1k`/`4k`/`16k`/`64k` item disk. A real disk
item can be installed from the player's hand. Ordinary held-item use on a Grid
inserts as many items as available drive capacity, preserving accepted counts.
The Grid lists component-distinct items and requests from digital disks.
Individual disk contents, installed tier, counts, network filters, and crafting
patterns are encoded in the original block entity's named-root NBT, committed
with `World::compare_entity` so concurrent changes fail closed. An ejected disk
retains its contents in the disk item's custom data. A storage part can upgrade
a lower-tier installed disk without losing those contents; the source's custom
`storage_disk_upgrade` crafting serializer still requires host support. Mining
a loaded Disk Drive in survival emits the installed disks with their contents.
Storage Blocks act as directly mounted bounded digital storage cells. Their
resources appear in the same Grid catalogue and can participate in one-block
atomic crafting. They save their contents on every insert/extract and through
restart. On survival mining, a nonempty Storage Block converts its digital
contents into a filled, tier-matched storage disk item, alongside the host's
ordinary empty block drop. A shared custom loot hook is needed to preserve
contents in a single storage-block item without minting a separate disk.

The **Crafting Grid** accepts nine ghost items by sneak-use while holding an
ingredient. Its menu lets the player select any of the nine 3×3 positions,
and a sneak-use with an empty hand skips a blank position, so shaped recipes
with holes or offsets are representable. It asks the host's live recipe
enumerator for the ordered vanilla and datapack matches in the same 3×3 grid.
When recipes conflict, the arrow cycles the actual results using the same
choice list as Polymorph. The selected recipe identity survives restart and
recipe-list reordering; if a datapack reload removes it, the live first match
is used. Clicking the displayed result consumes real digital-disk ingredients,
inserts the available item remainders and result, and
commits the entire operation in one drive or Storage Block's NBT
compare-and-update. This is deliberately limited to one storage block entity;
a recipe spread across multiple drives or external inventories is refused until the host
supports a multi-storage commit.

The **Pattern Grid and Autocrafter** provide real, bounded recipe automation.
The Pattern Grid records nine ghost ingredients, lets the player choose among
conflicting live recipes, and encodes the chosen recipe ID, serializer, exact
output components/count and grid into one `refinedstorage:pattern` item's
1,024-byte mod data. An Autocrafter holds up to nine installed Pattern
items and persists independent queues of at most 1,024 requested crafts per Pattern.
The host's live recipe enumeration is checked again on every craft, including
after datapack changes. Successful queued work consumes ingredients and
stores real recipe output/remainders in one Drive or Storage Block CAS. If
storage is unavailable, it leaves the job queued with all items untouched.
The Autocrafter also returns idle Pattern items on explicit menu ejection or
survival break.

### Refined Storage 1.12.4 Crafter subset

The Pattern Grid has a second, explicitly selected v1.12.4 PatternItem editor.
It records nine independent item input slots and nine output slots, retains
the source `Version`, `Processing`, and `Exact` flags, and writes source-named
`Input_0..8` / `Output_0..8` fields into the `refinedstorage:pattern` item's
bounded custom data. Inputs and outputs retain counts and item components.
Processing mode supports arbitrary item input/output mappings. Crafting mode
requires `Exact=true` and verifies the inputs against the current live recipe
manager; it derives the output from that recipe when the source item has no
`Output_i` preview field, or verifies the saved preview when present. Non-exact
ingredient alternatives are rejected because the SDK does not expose the
source's reverse ingredient-tag expansion.
The parser rejects unknown pattern fields, fluid slots, and allowed-tag data
rather than dropping source requirements that the item-only ABI cannot
represent. The ordinary recipe-pattern editor remains available separately.

The v1.12.4 `crafter` has nine single-pattern slots, accepts the v1.12.4 exact
crafting-pattern and processing-pattern encodings plus the existing native
crafting-pattern encoding, and has four speed-upgrade
slots. Its update interval and maximum successful-update count match the
upstream table (10/8/6/4/2 ticks and 1/2/3/4/5 updates). The four source redstone
modes are persisted; pulse mode records its powered interval and unlocks on the
falling edge. Facing-connected Crafters resolve a single root container in the
same validated network; cycles fail closed. Installed patterns and upgrades
are returned on menu ejection or survival break; failed item spawning is
logged and restoration is attempted, although the host break callback cannot
veto vanilla block removal. The Crafter Manager lists connected Crafter outputs
and persists the v1.12.4 size and search-box mode settings.

An Autocrafter saves an in-flight flag *before* starting a storage mutation,
so a job interrupted by an uncertain process crash cannot silently replay
and duplicate outputs. Its stock menu exposes a manual abort for these
uncertain jobs. This is a fail-closed recovery mechanism: the cancellation
forfeits any unfinished queued work, and the host still needs a durable
cross-entity journal to finish such jobs automatically and exactly once.

**Importers and exporters** transfer between real vanilla containers and
connected External Storage faces. Their automation uses the new host
`containers::require_transfers()`: each physical source-slot transfer supplies
its **complete current source NBT as an exact preimage**, and re-reads after
every partial destination acceptance. The host returns the real moved count;
a rejected transfer leaves both slots alone. The source-facing inventory
cannot be its own destination. Import filters default to a deny-list and
exporters to an empty allow-list. Each port saves up to nine ghost filters,
redstone requirement, and up to four consumed speed/stack upgrades.
The network's automation tick index reconstructs from loaded block events
after a clean restart.

Each External Storage interface has its own saved stock menu for item
allow/deny filters, highest-first priority, insertion and extraction
permissions, and powered-only operation. A physical inventory exposed through
multiple interfaces appears once in the network catalogue. The automation
router checks both source extraction and destination insertion permissions
before invoking the host transfer. An empty allow-list denies every item, while
an empty deny-list permits every item. Tied priorities use stable coordinates;
an unavailable or unpowered interface is not offered a transfer.

The `wireless_grid` item binds to a specific Controller. A connected
`wireless_transmitter` must have consumed one or more `range_upgrade` items;
the port uses sixteen blocks of three-dimensional reach per upgrade, up to four.
Binding stores the exact controller dimension and coordinates in the held
item's saved data. Remote menu actions revalidate controller ownership,
transmitter connection and player range at click time, including after restart.

## Current boundaries

The v1.12.4 Crafter path is configuration and pattern persistence only; it is
not yet a processing-task executor. The Crafter Manager is a read-only pattern
catalogue plus persisted size/search settings, not the source crafting monitor.
Processing patterns are encoded and installed but are not submitted to a
machine, and exact-mode matching is retained but not executed. Fluid input /
output handlers and AllowedTags are unsupported and are rejected fail-closed.
The host still needs an atomic transaction spanning digital disk debit, target
machine insertion, and task checkpoints before processing jobs can safely run.

Source Refined Storage 2.x is a much larger system. In particular, the native
port does not yet implement FE controller energy, fluid disks,
full 3×3 native graphics and advanced filter variants,
wireless item energy usage, recursive autocrafting dependency graphs and CPU scheduling, security
manager, interdimensional network bridges, disk interfaces, constructor/
destructor operations, fuzzy ore normalization or multi-disk transactions.
Physical External Storage currently exposes the outside inventory to the
Grid catalogue, while the safe atomic importer/exporter path operates between
real physical slots. It cannot yet commit a physical↔virtual-disk transfer as
one atomic host transaction.
Wireless range currently uses a fixed per-upgrade distance because source
platform FE, permissions and configuration values are not exposed by the SDK.

Also, **atomic live transfers are not crash durability**. The host's new
transfer operation protects physical slot moves during a running session but
cannot commit a digital disk CAS together with a player's stack debit, item
entity delivery, or another drive's NBT in one durable journal. The digital
Grid item deposit/withdraw and disk ejection paths must be treated as
incomplete under a process kill at the exact crossing. No source data is
fabricated if a recipe or machine callback fails.

The SDK currently exposes item-level `Crafting::remainder(stack)`, not the
recipe-level `getRemainingItems(crafting_grid)` behavior used by Refined
Storage 1.12.4. The current Crafting Grid and Autocrafter preserve the
host-provided item remainder for each occupied cell, including repeated
ingredients, counts and components. Recipe-dependent remainders/byproducts
that depend on the complete input grid remain unsupported until the host
exposes a bounded recipe-grid remainder result. The current SDK also has no
generic Refined Storage Crafter pattern-provider interface for connected
machines, and no durable multi-block transaction for crafting inputs and
outputs spread across separate storage entities.

## Source and evidence

Upstream v1.12.4 classes consulted for the new Crafter subset include
`PatternItem`, `CraftingPatternFactory`, `CrafterNetworkNode`,
`CrafterManagerNetworkNode`, and `IGrid`. Upstream 2.0.9 classes consulted include
`ControllerBlockEntity`,
`AbstractDiskDriveBlockEntity`, `ItemStorageDiskItem`, `ItemStorageVariant`,
`AbstractExternalStorageBlockEntity`, `AbstractImporterBlockEntity`,
`AbstractExporterBlockEntity` and `AbstractGridBlockEntity`.
The native Storage Blocks follow `StorageBlockBlockEntity`, but persist
inside their own block entity in lieu of source UUID-indexed storage repository
support from the host.
The exact `ItemStorageVariant` constants are `1k=1024`, `4k=4096`,
`16k=16384` and `64k=65536`. The source progression JSONs are placed
without rewriting their recipe definitions; the 14 fallback `c:` tags are
separate additive compatibility resources.

The crate contains pure tests for unique-controller topology, disk capacity
and component-preserving persistence, content transfer on disk ejection and
upgrade, atomic crafting refusal when inventory is missing/full, malformed
saved NBT, 3×3 ghost patterns, empty allow-list behavior and exact-preimage
host-transfer behavior.
Autocrafter tests cover encoded Pattern identity, queued restart state,
duplicate rejection, real recipe matching, ambiguous conflict prevention,
and fail-closed uncertainty after a crash checkpoint. New v1.12.4 tests cover
processing-pattern serialization, all nine input/output slots, component
identity, unsupported fluids/allowed tags, editor persistence, Crafter pattern
slot bounds, speed-upgrade timing, four redstone modes, facing-chain root and
cycle rejection, and Crafter Manager settings.
Authored gameplay scenarios pending the prime's exclusive build/runtime gate:
`tests/scenarios/refinedstorage-disk-grid-restart.txt`,
`refinedstorage-crafting-grid-restart.txt`,
`refinedstorage-atomic-external-restart.txt`,
`refinedstorage-drive-break-preserves-disks.txt`,
`refinedstorage-wireless-grid-restart.txt`,
`refinedstorage-storage-block-restart.txt`,
`refinedstorage-external-priority-restart.txt`,
`refinedstorage-autocrafter-restart.txt`,
`refinedstorage-pattern-conflict.txt`,
`refinedstorage-polymorph-selection-restart.txt`,
`refinedstorage-shaped-pattern-gaps.txt`,
`refinedstorage-multiplayer-stale-grid.txt`, and
`refinedstorage-crafting-grid-reload-conflict.txt`,
`refinedstorage-processing-pattern-restart.txt`,
`refinedstorage-crafter-chain-modes.txt`, and
`refinedstorage-crafter-manager-multiplayer-stale.txt`. These scenarios are **not
claimed to pass until executed and reviewed**.

The targeted checks to run only after obtaining the exclusive gate are:

```powershell
cargo test -p refinedstorage --lib
scripts\pack-mod.ps1 -Crate refinedstorage
scripts\scenario.ps1 -Scenario tests\scenarios\refinedstorage-disk-grid-restart.txt -RunDir run\refinedstorage-disk-grid -Fresh
```
