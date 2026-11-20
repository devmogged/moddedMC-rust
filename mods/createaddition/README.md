# Create Crafts & Additions (partial)

Rust port in progress for **Create Crafts & Additions 1.7.2** (NeoForge Minecraft 1.21.1), by MRH (MIT).

## Runnable passive content

`createaddition.mmc` registers four passive items (electrum ingot, electrum nugget, electrum sheet and zinc sheet), the electrum storage block, four release crafting recipes, its original block loot table, and relevant common item/block and vanilla mining tags. Data were taken from the published 1.7.2 jar. Broad pickaxe and incorrect-tool tag lists are narrowed to the only block currently registered.

Package with `powershell -NoProfile -File scripts/pack-mod.ps1 -Crate createaddition`. Original textures/models remain in the separately obtained jar and are not committed; provide it through `MODDEDMC_ASSET_JARS` when testing.

The server scenarios `tests/scenarios/ws4-createaddition-electrum.txt` (Create installed alongside the addon) and `ws4-createaddition-crafting.txt` pass with the pinned shared engine binary. The latter verifies actual crafting table interactions (ingot to nine nuggets and reverse), custom output item identity, and player inventory persistence across restart. See `docs/ws/create-addons-content.md` for evidence. No original NeoForge server oracle or real-client capture has been performed.

## Source-checked logic, not player-usable machines

- `src/electricity.rs` matches released electric motor FE consumption/activation, redstone transitions, alternator FE generation and stress calculation.
- `src/energy_storage.rs` matches `InternalEnergyStorage`, capped FE input/output, uncapped internal transfers and its `energy` NBT field.
- `src/rolling_mill.rs` models the released 1.7.2 Rolling Mill's RPM-scaled timer, nine output slots, one-input consumption, missing-recipe retry, output insertion and `Timer` NBT. The source copper-ingot recipe fixture stays in `tests/fixtures/ws4-createaddition`; an unsupported rolling recipe is not installed as a runnable datapack entry.

The motor, alternator, cables, rolling mill and other machines are **not yet registered**, and full FE networks are not implemented. Their gameplay needs WS-0's inter-mod kinetic service (request `N:/mmc-coord/requests/WS-4-01.md`), a generic FE capability and further real-server parity tests. No original Create machine behaviors are claimed runnable merely because unit tests pass.

Verification: `cargo test -p createaddition`, `cargo fmt --package createaddition -- --check` and `cargo clippy -p createaddition --all-targets -- -D warnings`.
