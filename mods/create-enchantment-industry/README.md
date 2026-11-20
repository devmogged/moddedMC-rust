# Create: Enchantment Industry (partial)

Native Rust port in progress from the published **2.5.4 Fabric beta** for **Minecraft 1.20.1**, by DragonsPlus (LGPL-3.0-or-later). The currently available `2.5.4b` NeoForge source checkout uses the same original registry IDs; the exact 2.5.4 Fabric binary was inspected with `javap` for the plain item entries.

## Working passive content

`create-enchantment-industry.mmc` registers `create_enchantment_industry:blazes_enchanting_handbook` and `create_enchantment_industry:experience_cake_base` as ordinary stackable items. The source defines both using `Item::new`, with no in-air or block-use processing behavior. It also ships only their applicable `create:upright_on_belt` tag, adapted from the Fabric release's plural `data/create/tags/items/upright_on_belt.json`; entries for not-yet-registered items were deliberately excluded.

Build: `powershell -NoProfile -File scripts/pack-mod.ps1 -Crate create-enchantment-industry`.

The published release's original models and textures are loaded from the locally provided jar via `MODDEDMC_ASSET_JARS` and **not committed**. The original LGPL license text is bundled in `LICENSE.md` and inside the `.mmc`.

Verification: `cargo test -p create-enchantment-industry` (1/1), `cargo fmt --package create-enchantment-industry -- --check`, `cargo clippy -p create-enchantment-industry --all-targets -- -D warnings`, and `tests/scenarios/ws4-enchantment-industry-items.txt`. The live scenario confirms both real registry IDs via `/give` and `/clear`, and both item stacks survive restart.

## Pure experience arithmetic (not yet player-accessible)

`src/experience.rs` ports the **released Fabric 2.5.4 bytecode**, rather than silently using 2.5.4b's later NeoForge implementation. `ExperienceHelper.getExperienceForNextLevel` and `getExperienceForTotalLevel` retain the published i32 XP thresholds and overflow semantics (the newer source uses long arithmetic and clamps). `getExperienceForPlayer` uses Java's positive-infinity-tie rounding. `ExperienceHelper.getExperienceFromFluid`, `getFluidFromExperience` and `CEIFluidUnits` implement the original 81 raw Fabric fluid units per mB, exact multiplications and overflow checking. Six targeted arithmetic tests cover thresholds, high-level overflow, rounding, and unit conversion. They do not grant XP, repair items or transfer live fluid.

## Missing

This is **not** an enchantment-processing port. Blaze enchanters, printer, mechanical grindstone, XP fluid, enchanting templates, super-experience items and other contents still depend on source-matched processing, item components, Create services, fluid handlers and host callbacks. Their recipes are not shipped as dead data. Full original Fabric 2.5.4 oracle parity, 26.3 adaptation and stock-client visual checks remain outstanding. This addon must not be presented as complete.
