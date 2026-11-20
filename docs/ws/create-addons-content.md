# WS-4 content: Create Crafts & Additions electrum subset

Scope: incremental passive content for **Create Crafts & Additions 1.7.2**, NeoForge Minecraft 1.21.1. Built independently from the concurrent WS-4 machine behavior work. Do not treat this as complete addon parity.

## Source and content

Original release: `createaddition__neoforge-1.21.1-1.7.2.jar` (ignored local jar at `N:/mmc-wt/create-addons/cache/ws4/`). Original source: `D:/mc-ref/createaddition-latest/src/main/java/com/mrh0/createaddition/index/CAItems.java` and `CABlocks.java`; exact recipes and loot copied from the published jar. License: MIT (2024 MRH). Original images and textures are **not** committed.

The independent `createaddition.mmc` now registers four plain items: electrum ingot, electrum nugget, electrum sheet and zinc sheet; and the electrum block, using the original vanilla gold-block physical template. The release's four crafting recipes, original block loot and relevant common tags are packaged. The broad pickaxe and incorrect-tool lists are narrowed to the only registered block; absent machines are not falsely represented as live content.

The electric motor, alternator, cables, spools, rolling mill and other machines are still **not** registered. The pure math/storage/rolling-mill kernels on the parent WS-4 branch are separate and need host interfaces (WS-4-01 and energy capabilities).

## Verification

- `cargo fmt --package createaddition -- --check`: pass.
- `cargo test -p createaddition`: 16/16 pass after integrating the contemporaneous WS-4 rolling-mill work (8/8 before that merge). The combined three addon crates pass 53/53 tests.
- `cargo clippy -p createaddition --all-targets -- -D warnings`: pass.
- `scripts/pack-mod.ps1 -Crate createaddition`: pass, standalone `.mmc` includes license and 18 data resources.
- `tests/scenarios/ws4-createaddition-electrum.txt` on port 26044, using shared engine binary `d6e5e8d560`: pass. Both Create and CreateAddition installed. Server logs registered block and items and loaded 4 recipes, 1 loot table, 12 tags. The bot unlocks all 4 recipes, gives custom items, checks iron-tool mining, original block loot, and electrum block persistence after restart.
- `tests/scenarios/ws4-createaddition-crafting.txt` on port 26045: pass. The bot opens a vanilla crafting table, crafts a full 1-ingot -> 9-nugget recipe, then a 9-nugget -> 1-ingot recipe by filling all 9 input cells, shift-clicks both outputs and verifies exact custom registry IDs via `/clear` before and after restart.
- Logs: `N:/mmc-coord/notes/WS-4-createaddition-electrum-scenario.log` and `N:/mmc-coord/notes/WS-4-createaddition-crafting-scenario.log`. Do not mistake vanilla client carrier ids in bot slot packets for incorrect custom item IDs: both final registry IDs were verified with server commands.

## Remaining work and integration

Original Java NeoForge 1.21.1 oracle and real client rendering have not been run. Custom namespace `c:storage_blocks/electrum` is present in packaged JSON but the pinning engine's vanilla `/execute if block ... #c:...` command reports unknown tag; this needs investigation by the host/vanilla parity owner, not a mod workaround. Generic block tags `minecraft:mineable/pickaxe` and `minecraft:needs_iron_tool` do work in the scenario.

When merging, reconcile `mods/createaddition/src/lib.rs`, `Cargo.toml`, `README.md` and `Cargo.lock` with the concurrent main WS-4 branch, preserving its `rolling_mill` module and helper dependencies.

## Further M2 materials milestone

Source-verified and registered 12 additional plain `Item::new` entries from the same published `CAItems.java` and release jar: diamond grit; copper, iron, gold and electrum wires; copper, iron, gold, electrum and brass rods; unbaked and baked cake bases. The original model JSON exists for all twelve in the release. Neutral vanilla carriers preserve standard item stacks while the separately supplied jar provides the original artwork. No special wire/spool/food behavior is claimed.

Added 12 source-extracted common tag files for those item groups (five individual rod tags and their all-metal tag, four individual wire tags and their all-metal tag, and diamond dust). With the existing data, the server loads 4 crafting recipes, 1 block loot table and 25 tags.

The native `.mmc` built successfully; `cargo test -p createaddition` still passes 16/16 (rolling-mill tests included), and strict Clippy and formatting pass. `tests/scenarios/ws4-createaddition-materials.txt` passed on port 26046: all twelve IDs were created and matched by the server's registry using `/give` and `/clear`; copper wire and baked cake base were confirmed again after restart. Log: `N:/mmc-coord/notes/WS-4-createaddition-materials-scenario.log`.

These are source-defined items and tags, not machine parity. Rods, wires and grit need the appropriate Create rolling, filling and processing recipes and the inter-mod services before they can all be produced in survival gameplay.

## New Age passive-items milestone

Published reference: `create-new-age__1.2.0+mc1.21.1.jar` (ignored local release) and `D:/mc-ref/new-age-latest/common/src/main/java/org/antarcticgardens/cna/CNAItems.java`. The source is BSD-3-Clause (2023 Antarctic Gardens); the original license was copied as `mods/create-new-age/LICENSE.md`. Neither original art nor downloaded mod binaries are committed.

New `create-new-age.mmc` registers exactly ten `Item::new` entries from `CNAItems.java`: overcharged gold/iron/diamond, nuclear fuel, thorium, radioactive thorium, overcharged iron/golden sheets and blank/copper circuits. The release's two `create_new_age:nuclear/*` tags accompany nuclear fuel; special wire items, magnet blocks, reactors, motors, generator coils and other machines are not falsely registered. Their mechanics need WS-4-01 and additional host/electricity APIs.

Verified `cargo test -p create-new-age` 14/14, fmt and strict Clippy, and packaged `.mmc` with the original license and both tag files. `tests/scenarios/ws4-new-age-items.txt` passed on port 26047 with WS-0's pinned `d6e5e8d560` binaries: ten real registry IDs checked through server commands, both tags loaded, nuclear fuel and copper circuit saved/reloaded with the player's inventory. Log: `N:/mmc-coord/notes/WS-4-new-age-items-scenario.log`. Original NeoForge oracle and real-client rendering remain unverified.

## Three-mod compatibility

A scratch copy of the pinned `p1a-create.txt` ran with `create.mmc`, `createaddition.mmc` and `create-new-age.mmc` on WS-4 port 26048. PASS: the original Create shaft/cog/motor gameplay and signed 1:2 gearing, RPM change/restart, shaft-break stop, resource-pack handshake, and absence of registration failures. The scratch world was removed. Log: `N:/mmc-coord/notes/WS-4-three-mods-p1a.log`. This only proves those sampled vanilla/Create behaviors coexist; New Age electrical machines remain unavailable.

## New Age magnetic blocks and crafting

Source: New Age 1.2.0 `CNABlocks`, `ImplementedMagnetBlock.java`, `CNATags` and the published NeoForge release jar. Five source magnetic blocks are registered, with strength values 1/2/4/8/24 reflected in a source-grounded unit test and their vanilla magnet blockstates and original models. The magnet tag still includes `minecraft:respawn_anchor`, matching the released data. The five block loot tables and three original shaped crafting JSON recipes are packaged alongside the original relevant magnet/item tags, with the broad vanilla pickaxe tag narrowed to just registered blocks. The `redstone_magnet` crafting JSON was intentionally not installed because its Forge common ingredient tags are not supported by the pinned baseline. `magnetite_block` is not generated in the world by this increment.

The host's current content ABI exposes one physical vanilla template, so the original 5.0/7.0 hardness/blast values are approximated with `minecraft:iron_block` (5.0/6.0). This is not complete vanilla/mod parity. The strength lookup is tested as a pure value; no generator coil or Create service is connected.

Verification: `cargo test -p create-new-age` 15/15, strict Clippy, formatter, and `scripts/pack-mod.ps1 -Crate create-new-age` PASS. `tests/scenarios/ws4-new-age-magnets.txt` on port 26047 passed for all five placements, mining tags, original loot, three recipe unlocks and all five world blocks after restart; log `N:/mmc-coord/notes/WS-4-new-age-magnets-scenario.log`. `tests/scenarios/ws4-new-age-magnet-crafting.txt` passed on port 26047 for one completed vanilla crafting-table recipe, six overcharged iron plus three overcharged gold yielding four `create_new_age:layered_magnet` blocks and exact resulting item ID after a restart; log `N:/mmc-coord/notes/WS-4-new-age-magnet-crafting-scenario.log`. Stock client receives a paper carrier for these custom blocks; `/clear` independently verifies registry identity. Original NeoForge oracle and real-client visuals still missing.

## Enchantment Industry 2.5.4 Fabric beta passive items

Published reference: `create-enchantment-industry__2.5.4-fabric.jar` (Fabric 1.20.1, LGPL-3.0-or-later), latest release from the WS-4 survey. The `CEIItems.class` registry was inspected with JDK 26 `javap` from that exact binary, cross-checking `D:/mc-ref/enchantment-industry-latest/src/main/java/plus/dragons/createenchantmentindustry/common/registry/CEIItems.java` at newer `2.5.4b` NeoForge head for the two plain `Item::new` fields. The release's original item models exist for both items.

New independently loadable `mods/create-enchantment-industry` crate registers only `blazes_enchanting_handbook` and `experience_cake_base`. The original `create:upright_on_belt` tag, housed under the Fabric 1.20.1 plural `items` path, is converted to 1.21.1 singular `item` layout and narrowed to the one registered cake base. Enchanting templates, super-experience nugget, experience bucket, foiled cake and other nontrivial items remain unregistered. None of the published recipes can be exercised against current source-defined behavior and dependency set, so no inapplicable recipe payload is installed.

The 43.6 KB original LGPL license is shipped in `LICENSE.md`; original asset files remain local and ignored. `scripts/pack-mod.ps1 -Crate create-enchantment-industry`, `cargo test -p create-enchantment-industry` (1/1), formatting and strict Clippy passed. Live `tests/scenarios/ws4-enchantment-industry-items.txt` passed on port 26046, asserting the two exact custom IDs and inventory persistence across restart; evidence in `N:/mmc-coord/notes/WS-4-enchantment-industry-items.log`. Full Fluid/Enchanter/Blaze/Printer/Super XP behavior and original Fabric 2.5.4 differential oracle are still blocked/unimplemented.

## Five-mod coexistence

Using the published release jars as local asset inputs, a scratch `p1a-create.txt` loaded five independently packaged native mods simultaneously: `create.mmc`, `create-more-recipes.mmc`, `createaddition.mmc`, `create-new-age.mmc` and `create-enchantment-industry.mmc`. On WS-4 port 26048 with the pinned shared engine, all original Create P1a assertions passed: motor placement, 1:1 and 1:2 signed gear transmission, changing 16 to 24 RPM, save/restart returning correct 24 and 48 RPM, shaft break stopping the output line, and resource-pack handshake. All mods loaded without reported registration panic. This does not prove full addon mechanic coexistence; most processing and electricity are not yet wired. Evidence: `N:/mmc-coord/notes/WS-4-five-mods-p1a.log`. Scratch run removed.

## Central Kitchen 2.6.2b RecipeUnpacking kernel

Confirmed published 2.6.2b NeoForge Minecraft 1.21.1 binary contains `plus.dragons.createcentralkitchen.common.packager.RecipeUnpacking` with the `plan`, `finish`, `aggregate`, `orderPattern`, `multiplier`, `units`, recursive `assign` and nested `Plan.apply` methods, verified by `javap -p`. The current public source checkout declares 2.6.2b and was followed method by method. The complete original LGPL-3.0-or-later license is stored in the addon crate.

New logic-only `mods/create-central-kitchen/src/recipe_unpacking.rs` models complete package planning and atomic inventory insertion for up to 9 slots: source-preferred order context; equal-count occupied slots; one-unit layout before uniform bulk batches; overlapping ingredient matching with bitmask memo; component-aware merging; fail-closed overflow; and `Plan.apply` preflight before mutation. A test inventory implements the relevant `IItemHandler` operations.

`cargo test -p create-central-kitchen` passed 9/9, formatting and strict Clippy passed. This is only an `rlib`, no runtime content, recipes or native package. The actual Create packager and Farmer's Delight cooking bridges require inter-mod services and recipe/inventory callbacks; this module must not be described as a playable addon or parity-complete. Original NeoForge oracle, persisted world/bot scenarios and hot-path performance still pending.
