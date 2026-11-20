# Farmer's Delight storage crates

A partial native Rust port of official Farmer's Delight 1.21.1-1.3.4,
https://github.com/vectorwing/FarmersDelight/tree/1.21,
source revision dd8b9b0 (MIT). Includes only carrot, potato and beetroot
storage crates, their original block/item models and textures, six crafting and
unpacking recipes, loot tables, and axe mineability.

Java sources: ModBlocks.CARROT_CRATE, POTATO_CRATE, BEETROOT_CRATE
(new Block(Block.Properties.ofFullCopy(Blocks.OAK_PLANKS).strength(2,3).sound(WOOD)))
and matching ModItems (new BlockItem(..., basicItem())).
The three standard recipes each consume nine matching vanilla crops and produce
one crate; unpacking returns nine crops. enableVanillaCropCrates defaults to true
in Configuration.java. The NeoForge-only condition is omitted for the standard
vanilla 26.3 datapack reader; toggling it is not supported yet.

This is not full Farmer's Delight parity. All other crops, cooking, tools,
foods, decorations, and loot remain to be ported, and the original-mod oracle
comparison is still required.
