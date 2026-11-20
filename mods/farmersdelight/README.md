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
- Four plain items, as `new Item(basicItem())`: straw, canvas, tree bark
  and rice panicle.
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

## Cooking-pot recipe matcher (source verified, not active gameplay)

`src/cooking.rs` decodes the original 28 `farmersdelight:cooking` recipes from
`tests/fixtures/cooking/`, copied byte-for-byte from the MIT 1.3.4 upstream
release. It ports `CookingPotRecipe.matches`: exactly the same number of
nonempty slots as ingredients and one-to-one, order-independent assignment
across six slots. The bounded backtracking matcher correctly handles duplicate
items, overlapping item tags and the original NeoForge 1.21.1 compound (OR)
and difference (base-minus-subtracted) ingredient types. It preserves the
source's recipe-book tab, cooking time, experience and explicit container
remainder, without inventing the implicit output crafting remainder.

Eight unit tests validate all 28 original JSON fixtures, duplicates, tags,
compound/difference rules, invalid definitions and source defaults. A live
cooking pot, burn/cooking ticks, containers, menus, experience and item use
are **not implemented**. This module is source-verified infrastructure for
that work, not a registered cooking gameplay feature.

## Cutting-board inventory and persistence (tested logic, not active gameplay)

`src/cutting_board.rs` implements the original one-slot inventory's stack
merging, simulation, component equality, bounded extraction and per-cut
single-item consumption. It also reproduces the NeoForge 1.21.1
`ItemStackHandler` NBT structure (`Inventory.Size`, `Inventory.Items`,
`Slot: Int(0)`) together with `IsItemCarved`. The implementation was traced
against the official `neoforged/NeoForge` 1.21.1 branch and the mod's
`CuttingBoardBlockEntity.java`. Missing legacy save keys default to an empty
one-slot board as NeoForge does; invalid/duplicate slot data fails closed.

Seven native unit tests check these behaviors, component-sensitive merging,
no mutation on simulated operations, and round-trip persistence. Item stacks
are still read or written through the future generic host interaction and
container hooks. This layer does **not** make a cutting-board block available
in the world; tool damage, custom recipe use, comparator update and a live
restart scenario remain prerequisites for declaring it playable.

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
components. No food is registered until the generic WS-0 item-components
request is implemented and validated against Java gameplay.

## Remaining behavior

Cabbage, tomato and onion crate recipes, and the rice bag recipe, depend on
unported ingredients and are intentionally excluded until their item mechanics
can be represented exactly. Those four blocks can currently be obtained
through Creative or commands, placed, broken, and saved, but not crafted in
Survival. Straw harvesting, rice farming, bark extraction, cooking, foods,
tools, other crops, decorations and most loot are still missing.

Food items need generic default-component registration (coordination request
`WS-1-01`); substituting approximate vanilla food properties would change
hunger, saturation and consumption effects. Original NeoForge gameplay oracle
and stock-client appearance remain unverified. See `docs/ws/buja.md` for
the exact test scenarios and progress.
