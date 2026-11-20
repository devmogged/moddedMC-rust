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
- Ten original vanilla-format recipes: three vanilla-crop crates, their
  unpacking recipes, canvas, book from canvas, painting from canvas and
  scaffolding from canvas.

The carrot/potato/beetroot crate recipes consume nine corresponding vanilla
crops and unpack to nine. `enableVanillaCropCrates` defaults to true in
`Configuration.java`; its NeoForge-only recipe condition was removed because
the vanilla 26.3 datapack reader does not implement NeoForge conditions. The
configuration toggle is not yet ported.

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
