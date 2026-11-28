# Create survival gaps

Compared with Create 6.0.10 (`mc1.21.1-6.0.10`). The inventory uses `AllBlocks`, `AllItems`,
`AllPaletteBlocks`, `AllPaletteStoneTypes` and the release's generated blockstate and recipe IDs.
The port side uses the host's registered block and item IDs, the Create behavior modules and
the data files bundled in `create.mmc`.

Create registers 476 blocks and 56 standalone items. Of 643 released blockstate IDs,
171 remain outside the port. The package includes 1,495 recipe files and 468 block loot tables.
The release generates 1,884 recipe files and 640 block loot tables. A bundled file may still need
its runtime mechanic or an optional mod to run.

| Order | What a survival player hits |
| --- | --- |
| 1 | **Steam power.** Steam Engines form boilers, consume pumped water, and share capacity across shafts. The tested boilers restore their output and retain engine attachments across chunk loads and restart. Boiler gauges show the water and heat limits on exposed sides. Steam Whistles attach to the top or side of tanks, grow and retune, and sound on redstone with heat. Goggles pitch information, Create's steam jet, advancements and continuous client audio still need work ([issue #30](https://github.com/devmogged/moddedMC-rust/issues/30)). |
| 2 | **Building and materials.** Full stone palettes, pillars, framed glass, solid windows, and 56 each of stone slabs, stairs and walls craft or stonecut and drop. Slabs merge, stairs form corners with vanilla stairs, and walls connect to stock blocks and respond to partial blocks overhead. All three waterlog and use their release material settings on the server. Stock-client sounds, wall overlay seams and frame timing remain under [issue #32](https://github.com/devmogged/moddedMC-rust/issues/32), [issue #37](https://github.com/devmogged/moddedMC-rust/issues/37) and [issue #38](https://github.com/devmogged/moddedMC-rust/issues/38). All 18 window and glass panes craft or stonecut, connect to stock panes and bars, waterlog, and move on contraptions. Their connected textures and stock-client seams are tracked in [issue #39](https://github.com/devmogged/moddedMC-rust/issues/39). Andesite, Brass and Copper Bars stonecut, connect to stock panes and bars, waterlog, and support sneak-wrench pickup and contraptions ([issue #40](https://github.com/devmogged/moddedMC-rust/issues/40)). All three metal ladder families stonecut, waterlog, hang from matching ladders, and support vertical placement assist and wrench use ([issue #42](https://github.com/devmogged/moddedMC-rust/issues/42)). Andesite, Brass and Copper Scaffolding stonecut, hang in midair, extend from a matching stack, waterlog, and support sneak-wrench pickup. The companion uses the same standing and descent rules as the server and stitches matching platform textures. Real-client verification is pending ([issue #56](https://github.com/devmogged/moddedMC-rust/issues/56)). All 48 copper shingle and tile blocks, slabs and stairs craft or stonecut, oxidize alongside vanilla copper, and support waxing, axe scraping, lightning and contraptions. Connected roof textures still need client rendering work ([issue #41](https://github.com/devmogged/moddedMC-rust/issues/41)). Train and Framed Glass Trapdoors craft, waterlog, respond to hand and redstone use, and support sneak-wrench pickup. Stationary Framed Glass Trapdoors use connected textures through the companion. Both trapdoors assemble into contraptions, leave source water behind, and reevaluate water when they land. Both materials toggle on contraptions, with display, collision and save state following the change. The client checklist covers the moving hit target ([issue #58](https://github.com/devmogged/moddedMC-rust/issues/58)). Sliding Doors still need their ordinary hand and redstone behavior. Metal Girders, encased cogs, colored sails, Table Covers, Cardboard Blocks and Copycat Step, Panel and Bars remain. |
| 3 | **Item storage and control.** Funnels, Chutes, Tunnels, the Mechanical Arm, Threshold Switch and Smart Observer cover basic routing and sensing. Redstone Links carry analog strength between matching frequency pairs within 256 blocks, power the block they hang on, and keep their mode and frequencies across restart ([issue #64](https://github.com/devmogged/moddedMC-rust/issues/64)). Item Vaults, the Linked Controller and lectern controller, links on contraptions, Item Hatches, Factory Gauges, Redstone Requesters, Creative Crates, Toolboxes, analog levers and brass diodes remain. Attribute Filter creative-tab matching and special Arm targets sit behind these paths. |
| 4 | **Farm, belts and processing.** Stationary Drills, stationary and mounted Saws, moving Harvesters, and piston-mounted Ploughs cover tested jobs. Ploughs still need falling-block handling, the flying-anvil advancement and train-mounted actors ([issue #29](https://github.com/devmogged/moddedMC-rust/issues/29)). Mounted Deployers, Portable Storage and Fluid Interfaces, Smart Fluid Pipes, and more custom filling and sequenced recipes remain. Belt slicing, dyes, stock-client slope transport, and full bulk blasting and smoking coverage remain. |
| 5 | **Packages, displays and notes.** Packagers and Repackagers, Frogports and Postboxes, Chain Conveyors, Stock Links and Stock Tickers, address filters and Shopping Lists remain. Display Links and Boards, Nixie Tubes, Clipboards, Placards, bells, and lectern controls remain. |
| 6 | **Equipment and tools.** Engineer's Goggles and Ponder, Extendo Grip, Potato Cannon, Diving Gear and Backtanks, Cardboard armor and sword, Symmetry Wand, Tree Fertilizer, Nugget and Block of Experience, Builder's Tea and the other food items remain. |
| 7 | **Schematics.** Schematic and Quill, Schematic editing and placement, Schematic Table and Schematicannon remain. Empty Schematics are available as an item. |
| 8 | **Contraptions and rail upgrades.** Pistons, bearings, pulleys, elevators, seats, assembled train driving, occupied signals and two-stop schedules work in the checked cases. Cart Assemblers and minecart contraptions, train Observers and station editing, schedule conditions, Sequenced Gearshifts, contraption controls, standing-player carrying and double-chest capture remain. |

Stone slabs, stairs and walls move on contraptions and retain their stock-client models. Stair corners and wall arms update against their new neighbors on disassembly.

The 56 walls add 18,144 complete block states. A fresh Create-only world reaches runtime state 60,805, leaving 4,730 IDs in the current 16-bit range. In two matched 4,096-wall placement runs, the wall overlays used 417–423 MiB server RSS and sent 1.124–1.126 MB of captured payload per bot. The stock walls used 387–395 MiB and sent 42–50 KB. The stock-client frame and seam checks are in [CLIENT-CHECKS.md](../CLIENT-CHECKS.md).

The 18 pane families add 576 states, bringing the Create-only runtime maximum to 61,381.
The three framed and tiled textures using spare waxed copper bar states spawn no placed entities.
In two 4,096-pane captures, those carriers sent 54–55 KB per bot at 369–395 MiB server RSS.
Vanilla panes sent 39–44 KB at 388–394 MiB RSS. The Oak Window display path sent
1.044–1.047 MB at 406–426 MiB RSS with 4,096 displays. These runs measured packets
and server memory. The frame timing and transparency checks need the official client.

The three metal bar families add 96 block states and use one display per placed block.

## Recipe gaps by serializer

- Crafting, stonecutting and block loot follow the missing IDs above. The palette pass includes
  recipes whose ingredients, tags and results resolve today. Optional cross-mod inputs still
  depend on the appropriate registrations and conditions.
- Remaining Create serializers and paths include custom filling, general fluid emptying,
  schematic copying, Toolbox dyeing, and machine or item variants of deployment, pressing,
  mixing, compacting, item application, sequenced assembly and mechanical crafting.
- Furnace, Blast Furnace, Smoker, Campfire and Smithing data still need their missing Create item
  registrations and optional ingredient checks.

## Deferred

Palette models use a fixed variant and retain their seams ([issue #28](https://github.com/devmogged/moddedMC-rust/issues/28)).
Other visual parity includes exact
particles and sounds, advancement triggers, animation timing and stock outlines. Track timing,
stress flicker and detailed item-display offsets sit behind the survival paths above.

Sail dyes are tracked in [issue #14](https://github.com/devmogged/moddedMC-rust/issues/14).
Linked Crafter casing textures are tracked in [issue #15](https://github.com/devmogged/moddedMC-rust/issues/15).
The Rotation Speed Controller cog bracket is tracked in [issue #20](https://github.com/devmogged/moddedMC-rust/issues/20).
Chain casing mining hardness is tracked in [issue #21](https://github.com/devmogged/moddedMC-rust/issues/21).
Andesite Tunnel flap animation and Brass Tunnel textures are tracked in [issue #25](https://github.com/devmogged/moddedMC-rust/issues/25)
and [issue #26](https://github.com/devmogged/moddedMC-rust/issues/26).
Stationary Drill cracks and hit sounds are tracked in [issue #27](https://github.com/devmogged/moddedMC-rust/issues/27).
