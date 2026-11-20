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
