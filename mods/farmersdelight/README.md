# Farmer's Delight native port

A partial Rust port of Farmer's Delight 1.21.1-1.3.4 from
https://github.com/vectorwing/FarmersDelight/tree/1.21,
revision `dd8b9b0` (MIT). All included models, textures, recipes and loot
tables come from that version's source repository. The original license is
included as `LICENSE.md`.

## Implemented

- All six crop crates: carrot, potato, beetroot, cabbage, tomato and onion.
  `ModBlocks.java` registers these as ordinary blocks with
  `Block.Properties.ofFullCopy(Blocks.OAK_PLANKS).strength(2, 3).sound(WOOD)`.
  The matching `ModItems.java` entries are ordinary `BlockItem` instances.
- Rice bag, a plain block with `Block.Properties.ofFullCopy(Blocks.WHITE_WOOL)`.
- Straw, canvas, tree bark, rice panicles and raw rice.
- Raw cabbage, tomato and onion can be eaten with their original hunger,
  saturation and consumption timing.
- The released recipes for packing and unpacking the cabbage, tomato and
  onion crates and rice bag, and for turning rice panicles into rice.
- Original block models, textures, loot tables and axe mineability tags.
- Thirteen original vanilla-format recipes: three vanilla-crop crates and
  their unpacking recipes; canvas, book, painting and scaffolding from canvas;
  lead from straw, packed mud from straw and mud, and paper from tree bark.

The carrot/potato/beetroot crate recipes consume nine corresponding vanilla
crops and unpack to nine. `enableVanillaCropCrates` defaults to true in
`Configuration.java`; its NeoForge-only recipe condition was removed because
the vanilla 26.3 datapack reader does not implement NeoForge conditions. The
configuration toggle is not yet ported.

## Cutting-board recipe evaluator (preparatory, not active gameplay)

`src/cutting.rs` ports the original `CuttingBoardRecipe` input/tool-matching
rules and `ChanceResult.rollOutput` semantics. Its 72 source-backed JSON
fixtures live under `tests/fixtures/cutting/` (official 1.3.4 release,
revision `dd8b9b0`, MIT); the fixture test parses every recipe, including
item-ability alternatives, chance outputs, and optional sound identifiers.
Five targeted tests cover Java float-to-double probability promotion,
per-unit draw consumption, fortune chances, invalid definitions, and exact
tool alternatives. Production board block registration and callbacks are not
wired up yet: tool wear, item-ability queries, and block inventory capability
need the generic host interfaces requested in `WS-1-03`. Compiling this recipe
layer does **not** mean the cutting board works in Survival yet.

## Source-verified food values (reference only)

`food_values.json` records **74** FoodProperties constants and three vanilla
soup overrides from the exact official `FoodValues.java` source. The original
MIT Java fixture is preserved under `tests/fixtures/FoodValues.java` and can
be independently checked with:

```powershell
py -3 mods/farmersdelight/tools/extract_food_values.py --check
```

`src/food_values.rs` exposes a typed, once-parsed lookup with tests covering
cabbage/tomato/onion, nourishment and Hunger effects, chance/duration, fast
food, always-edible food and bowl remainders. The manifest retains literal
Java saturation/chance values. The unusual `KELP_ROLL` direct constructor
remains explicitly unresolved; missing builder fields are `null`, not guessed
defaults. These are **Java 1.21.1 reference values**, not ready-made 26.3
components. The raw crop items use the item-defaults ABI to translate the
recorded nutrition and saturation modifiers into 26.3 food components.

## Remaining behavior

Natural crop acquisition, growth and harvesting still need a gameplay port,
including planting with onion and rice items. Straw harvesting, bark extraction,
cooking, prepared foods, tools, decorations and most loot are still missing.
The original NeoForge gameplay oracle and stock-client appearance remain
unverified. See `docs/ws/buja.md` for
the exact test scenarios and progress.
