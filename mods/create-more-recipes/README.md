# Create More Recipes for moddedMC

Partial native Rust port of Create More Recipes `1.2.1-fix1` (NeoForge 1.21.1), built as the separate `create-more-recipes.mmc` binary. Original project by iglee42, MIT licensed.

## What currently works

- `cmr:snowman_cooler`: all 16 facing/heat states, player item fueling, regular and special cooling, creative fuel logic, and the fuel timer saved as original Java NBT (`fuelLevel`, `burnTimeRemaining`, `isCreative`). Existing `Goggles` and `TrainHat` NBT fields survive loading and saving.
- Regular fuel via source-defined item tags: snow block, snowball, packed ice, blue ice, powder-snow bucket. Powder snow returns its empty bucket.
- `cmr:frozen_cake` special fuel, and the `cmr:frozen_cake_base` item registered separately.
- `cmr:empty_snowman_cooler`, its original recipe, block loot for both coolers, and their mineable pickaxe tag. The original empty cooler loot table drops a filled cooler.

Build and pack on Windows from the repo root:

```powershell
powershell -NoProfile -File scripts/pack-mod.ps1 -Crate create-more-recipes
```

Drop `target/mods/create-more-recipes.mmc` in the server's `mods/`. To display original art, supply a local copy of the addon jar via `MODDEDMC_ASSET_JARS`. The `.mmc` has no bundled original textures.

Run unit tests and checks:

```powershell
cargo test -p create-more-recipes
cargo fmt --package create-more-recipes -- --check
cargo clippy -p create-more-recipes --all-targets -- -D warnings
```

Run `tests/scenarios/ws4-cooler.txt` on a WS-4 port using WS-0's shared `pack-dev.exe` and `moddedmc-bot.exe`. It checks block and datapack registration, mineable tags, original empty-cooler loot, regular/special fuel consumption, powder-snow bucket remainder, and the original saved NBT through a restart. A scratch copy of Create's `p1a-create.txt` with both `.mmc` files loaded passed on port `26042`. Logs are in `N:/mmc-coord/notes/WS-4-*.log`.

## Still missing

- Basin recipes with `cooled`/`frozen` heat, snowman-cooler-driven mixing, the Smithing Press and Block Spout runtime, custom fan processing, and remaining recipe JSON. WS-4-02 asks the Create processing owner for the necessary service hook. **Smithing Press's 240-tick belt processing cycle and Java NBT** are independently modeled in `src/smithing_press.rs` (including source-matched RPM speed, half-cycle timing and template/addition input conservation). **Block Spout's state machine** is independently modeled in `src/block_spout.rs` (100-tick processing cycle, 1,000 mB tank, dirt + 500 mB water → grass block, source-matched NBT and restart semantics). Its original 1.2.1-fix1 recipe is preserved under `tests/fixtures/ws4-create-more-recipes/grass_block.json` for a data-driven test, not installed as an unsupported recipe.
- `Cooler::snowball_impact()` already ports and unit-tests the original +80 normal fuel ticks (capped at 10,000) and ignoring creative/special fuel. The actual thrown snowball event, cancellation and sound await WS-4-03, so it is **not** yet player-accessible.
- Capturing snow golems or spawner entities using the empty cooler. The source-grounded thrown-snowball state transition is tested but still needs the projectile-impact callback. Both behaviors require generic host hooks requested in WS-4-03.
- Full renderer, goggles interaction, animations, contextual collision shape, sound/particles, modded crafting dependencies, and an original 1.21.1 NeoForge differential oracle run.

Passing the live bot scenario only verifies the implemented subset. It is not a claim of complete parity with the original addon.
