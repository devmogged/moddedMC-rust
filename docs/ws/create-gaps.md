# Create survival gaps

Compared with Create 6.0.10 (`mc1.21.1-6.0.10`). The inventory uses `AllBlocks`, `AllItems`,
`AllPaletteBlocks`, `AllPaletteStoneTypes` and the release's generated blockstate and recipe IDs.
The port side uses the host's registered block and item IDs, the Create behavior modules and
the data files bundled in `create.mmc`.

Create registers 340 blocks and 56 standalone items. Of 643 released blockstate IDs,
307 remain outside the port. The package includes 1,260 recipe files and 335 block loot tables.
The release generates 1,884 recipe files and 640 block loot tables. A bundled file may still need
its runtime mechanic or an optional mod to run.

| Order | What a survival player hits |
| --- | --- |
| 1 | **Steam power.** Steam Engines form boilers, consume pumped water, and share capacity across shafts. The tested boilers restore their output and retain engine attachments across chunk loads and restart. Boiler gauges show the water and heat limits on exposed sides. Steam Whistles attach to the top or side of tanks, grow and retune, and sound on redstone with heat. Goggles pitch information, Create's steam jet, advancements and continuous client audio still need work ([issue #30](https://github.com/devmogged/moddedMC-rust/issues/30)). |
| 2 | **Building and materials.** Full stone palettes, pillars, framed glass, solid windows, 56 stone slabs and 56 stone stairs craft or stonecut and drop. Slabs merge and stairs form corners with vanilla stairs. Both waterlog and use their release material settings on the server. Stock-client sounds and frame timing remain under [issue #32](https://github.com/devmogged/moddedMC-rust/issues/32) and [issue #37](https://github.com/devmogged/moddedMC-rust/issues/37). Walls, panes and bars need placement, neighbor shapes and multipart models. Copper slabs wait on oxidation and waxing. Metal Ladders and Scaffolds need climbable stock carriers and support rules. Copper shingles and tiles need oxidation and waxing. Sliding Doors and Trapdoors need their ordinary hand and redstone behavior. Metal Girders, encased cogs, colored sails, Table Covers, Cardboard Blocks and Copycat Step, Panel and Bars remain. |
| 3 | **Item storage and control.** Funnels, Chutes, Tunnels, the Mechanical Arm, Threshold Switch and Smart Observer cover basic routing and sensing. Item Vaults, Redstone Links and the Linked Controller, Item Hatches, Factory Gauges, Redstone Requesters, Creative Crates, Toolboxes, analog levers and brass diodes remain. Attribute Filter creative-tab matching and special Arm targets sit behind these paths. |
| 4 | **Farm, belts and processing.** Stationary Drills, stationary and mounted Saws, moving Harvesters, and piston-mounted Ploughs cover tested jobs. Ploughs still need falling-block handling, the flying-anvil advancement and train-mounted actors ([issue #29](https://github.com/devmogged/moddedMC-rust/issues/29)). Mounted Deployers, Portable Storage and Fluid Interfaces, Smart Fluid Pipes, and more custom filling and sequenced recipes remain. Belt slicing, dyes, stock-client slope transport, and full bulk blasting and smoking coverage remain. |
| 5 | **Packages, displays and notes.** Packagers and Repackagers, Frogports and Postboxes, Chain Conveyors, Stock Links and Stock Tickers, address filters and Shopping Lists remain. Display Links and Boards, Nixie Tubes, Clipboards, Placards, bells, and lectern controls remain. |
| 6 | **Equipment and tools.** Engineer's Goggles and Ponder, Extendo Grip, Potato Cannon, Diving Gear and Backtanks, Cardboard armor and sword, Symmetry Wand, Tree Fertilizer, Nugget and Block of Experience, Builder's Tea and the other food items remain. |
| 7 | **Schematics.** Schematic and Quill, Schematic editing and placement, Schematic Table and Schematicannon remain. Empty Schematics are available as an item. |
| 8 | **Contraptions and rail upgrades.** Pistons, bearings, pulleys, elevators, seats, assembled train driving, occupied signals and two-stop schedules work in the checked cases. Cart Assemblers and minecart contraptions, train Observers and station editing, schedule conditions, Sequenced Gearshifts, contraption controls, standing-player carrying and double-chest capture remain. |

Stone slabs and stairs move on contraptions and retain their stock-client models. Stair corners still need to update against their new neighbors on disassembly.

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
