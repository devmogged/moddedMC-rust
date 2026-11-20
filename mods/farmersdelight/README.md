# Farmer's Delight storage crates

A partial native Rust port of official Farmer's Delight 1.21.1-1.3.4,
https://github.com/vectorwing/FarmersDelight/tree/1.21,
source revision dd8b9b0 (MIT). Includes only carrot, potato and beetroot
storage crates and four basic ingredients (straw, canvas, tree bark, rice
panicle), original models/textures, ten crafting recipes, three loot tables,
and axe mineability.

Java sources: ModBlocks.CARROT_CRATE, POTATO_CRATE, BEETROOT_CRATE
(new Block(Block.Properties.ofFullCopy(Blocks.OAK_PLANKS).strength(2,3).sound(WOOD)))
and matching ModItems (new BlockItem(..., basicItem())).
The three standard recipes each consume nine matching vanilla crops and produce
one crate; unpacking returns nine crops. enableVanillaCropCrates defaults to true
in Configuration.java. The NeoForge-only condition is omitted for the standard
vanilla 26.3 datapack reader; toggling it is not supported yet.

The additional basic items are declared with new Item(basicItem()) in ModItems.java.
Canvas, book, painting, and scaffolding recipes are copied from the official
release. Straw harvesting, rice harvesting, tree bark extraction, crop growth,
cooking, tools, foods, decorations, and most loot are not implemented. Original
mod gameplay oracle comparison is still required; this is not full parity.
