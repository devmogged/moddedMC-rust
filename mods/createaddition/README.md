# Create Crafts & Additions (partial)

Rust port in progress for Create Crafts & Additions 1.7.2, NeoForge Minecraft 1.21.1, by MRH0 (MIT).

## Native content available

The createaddition.mmc now registers four passive items (electrum ingot, electrum nugget, electrum sheet and zinc sheet), the electrum storage block, four released vanilla crafting recipes, its original block loot table, and the relevant common-namespace item/block tags and mining requirements. Their original data were taken from the 1.7.2 release jar. Only pickaxe/incorrect-tool tag lists were narrowed to the single block that is actually registered; no missing machines or items have been declared as supported.

The original client models/textures are not included in this repository or .mmc. The server may load them from the separately obtained release jar through MODDEDMC_ASSET_JARS.

To build from the repository root:

    powershell -NoProfile -File scripts/pack-mod.ps1 -Crate createaddition

The integration scenario tests/scenarios/ws4-createaddition-electrum.txt checks the addon with Create loaded, recipe registration, placement/loot, tags, and restart. The additional tests/scenarios/ws4-createaddition-crafting.txt performs both recipes in the crafting table and verifies the custom items stay in player inventory after restart. Original NeoForge 1.21.1 oracle comparison and client rendering remain outstanding.

## Source-checked mechanics, not yet gameplay-enabled

src/electricity.rs ports the electric motor's FE consumption/activation, redstone transitions and the alternator's FE/stress calculations. src/energy_storage.rs ports InternalEnergyStorage, including bounded FE input/output, uncapped internal transfers and its saved energy NBT key. The relevant Java methods were checked against the published 1.7.2 binary using javap -c -p and public source.

Electric motor, alternator, cables, rolling mill, electricity, spool use, and other machine behaviors are not registered. They require the named Create kinetic service (request N:/mmc-coord/requests/WS-4-01.md), generic energy handlers and further parity tests. Create's pressing recipes are also intentionally held back until pressing is running in the pinned Create baseline.

Unit tests: cargo test -p createaddition; lint: cargo clippy -p createaddition --all-targets -- -D warnings. Original model rendering and full 1.21.1 NeoForge differential parity remain outstanding.
