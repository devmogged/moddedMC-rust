# Java 26.3 client checks

Run these checks with the official client and the server resource pack.

## Mounted Mechanical Saw (`db90e12`)

- Mount a Saw sideways on a sticky piston with a chest and point it at an oak tree. Power the piston. Expect the Saw blade and contraption to move together, the Saw to pause as it cuts, and the tree drops to go into the chest.
- Restart while the Saw is cutting. Expect it to resume and reach the same end position. Put a log in its path and reverse the piston. Expect the log to remain intact when the Saw faces away from it.

## Create palettes (`844a260`)

- Craft or stonecut asurine, crimsite, limestone, ochrum, scoria, and veridium variants. Expect the recipe outputs and placed textures to agree. Break the blocks with the appropriate tool and check that the matching items drop.
- Place pillar blocks against the floor, wall, and ceiling. Expect their grain to follow the placement axis. Build a wall with wood-framed windows and check the glass, frames, and item icons.

## Opaque palette carriers (`82ed195`)

- Build a floor and walls from opaque Create stone with glass beside them. Expect the stone to appear opaque, cast shadows, and move with the chunk when you reconnect. Check for visible vanilla leaf models or particles while looking at and breaking the blocks.
- Walk and sprint past a large palette build, then leave and re-enter its chunk range. Expect distant blocks to appear as their chunks arrive. Check for obvious stalls while they load.

## Mechanical Plough (`9810569`)

- Attach a Plough to each side of a solid block. Expect it to face outward and have its scoop against the ground. Try the same placement against the side of a leaf block. Expect the item to stay in your hand.
- Mount an east-facing Plough on a sticky piston with a chest. Extend the piston over dirt, layered snow, and wheat on farmland. Expect tilled dirt, snowballs in the chest, and the wheat intact. Put an armor stand in the active path and expect the Plough to push it forward.
- Restart during the extension. Expect motion and snow collection to resume and the Plough to settle at the far end. Clear the retraction path, put fresh dirt below it, and reverse the piston. Expect it to return without tilling the dirt.


## Steam Engine and Powered Shaft (`7abe38e`)

- Build a four-block Fluid Tank boiler with magma below it. Attach a Steam Engine to the side and use a Shaft on it while looking across the engine's axis. Expect the Shaft to appear two blocks outward on the perpendicular axis and the tank windows to close.
- Pump water into the boiler. Expect the exposed shaft and its connected machines to turn. Replace the magma with a fueled Blaze Burner. Expect the moving shaft to speed up with the crank, linkage, and piston moving together. Inspect the stroke on side, floor, and ceiling installations.
- Restart with the pump running. Expect rotation to resume. Use a Wrench on the engine to reverse it, then stop the pump. Expect motion to stop after the boiler's recent water supply runs out.
- Remove the engine. Expect its piston, linkage, and connector to disappear and the shaft to remain. Repeat with a waterlogged shaft and check that the water remains in place.


## Boiler gauges (e20f71c)

- Form a boiler with four tank blocks, passive heat, and a Steam Engine. Expect a gauge to sit against each exposed side of the bottom tank layer. Pump water into the boiler and watch the needle rise. Add active heat and expect a higher reading.
- Cover one gauge with a solid block. Expect the other three to stay visible. Restart and remove the obstructing block. Expect the fourth gauge to appear again. Remove the last engine and expect all of the gauges to disappear.

## Steam Whistle (`8905d60`)

- Craft two Steam Whistles. Place one on top of a Fluid Tank and the other on a side of a separate tank. Expect both to point away from their supports and show the correct floor and wall bases. Power both with redstone while the tanks are cold. Expect silence.
- Put magma under both tanks. Expect the mouths to rise and vibrate while the whistles sound. Use a held Steam Whistle repeatedly on one pipe, then use a Wrench to cycle its octave. Expect half-block extensions, a lower pitch as the pipe grows, and each octave's own tone. Check the sound through several sample repeats.
- Cut power to just one whistle. Expect its mouth to settle and its tone to stop independently of the second. Switch it back on, reconnect, and expect both tones to resume with their saved pitches. Move away and back while they remain powered to check how playback resumes at tracking distance.
- Take away one supporting tank. Expect the whistle and its extensions to come down, its displays to disappear, and its sound to stop. Check both support orientations.

## Stone palette slabs (`64f999b`, `0e079bb`)

- Craft and stonecut Cut Andesite Slabs and Cut Calcite Slabs. Expect each to use its own texture. Place one against the lower half of a face and the other against the upper half. Expect each to occupy the half you clicked. Walk onto the lower slab and under the upper slab to check collision.
- Place the same slab on its exposed half. Expect a full double slab and two slabs when mined with a pickaxe. Use two slabs in the recycling recipe and expect one matching full block.
- Pour water into a single slab, and place another slab into a water source. Expect both to hold water through restart. Merge a waterlogged single slab and expect the double slab to lose the water. Use a bucket on a dry double slab and expect water to pour beside it.
- Build a large mixed floor of Cut Andesite and Cut Calcite slabs. Look from both above and below, pan past the floor, then leave and re-enter its chunks. Expect the correct half-block models to stay aligned with collision. Capture frame timing and any pop-in from the display-backed Calcite slabs.
- Place Cut Calcite and Cut Dripstone slabs, walk over them, and mine them with the same pickaxe. Expect each to sound like its source stone and to drop one slab. Compare placement, footsteps, and breaking with the corresponding vanilla source blocks. Record any sound that follows the carrier block.

## Stone palette stairs (Pumpkin `f040199c4`)

- Craft Cut Andesite Stairs at a crafting table and stonecut Cut Calcite Stairs. Expect four Cut Andesite Stairs per crafting recipe and one Cut Calcite Stair per stonecutting input. Place both upright and upside down in each horizontal direction. Expect the treads to align with collision and the texture to stay upright on the exposed faces.
- Join each stair with vanilla Stone Stairs in both directions. Expect inner and outer corners to appear on either material. Remove the neighboring stair and expect the remaining one to straighten. Repeat upside down. Check the corner seams and walk over each joint.
- Place one of each material in water and mine another with a pickaxe. Expect a water source inside the placed stair and one matching stair from the mined block. Restart and expect the water, facing, and neighboring corners to remain.
- Build equal large stair arrays from Cut Andesite and Cut Calcite. Pan across them and leave and re-enter their chunks. Expect all models to stay aligned, with the same result after reload. Capture frame timing and any display pop-in or carrier-dependent sounds.

## Moving stone slabs and stairs (`b46d430` and `5916765`)

- Glue a Cut Calcite Slab and an upside-down Cut Calcite Stair to a Mechanical Bearing assembly. Power it and expect both blocks to keep their textures and face orientation as they rotate, with no stationary copies left at the source. Restart while the bearing is moving, then stop it. Expect the blocks to remain visible and come to rest as ordinary slabs and stairs.
- Move a waterlogged stair away from its source. Expect water where the stair started and a dry stair when you disassemble onto dry ground. Repeat over a water source and expect the placed stair to waterlog again.
- Assemble a stair that forms an outer corner with a fixed vanilla stair. Remove the fixed stair while the bearing runs, then stop the bearing. Expect the moved stair to straighten. Repeat with a fixed stair that makes an inner corner at the landing position. Expect the corner to form on disassembly and persist after restart.

## Vanilla wall shapes (Pumpkin `101e2f325`)

- Place a Cobblestone Wall between two Stone blocks and add a bottom slab above the wall. Expect both arms to rise and the center post to stay down. Swap the bottom slab for a top slab and expect both arms to lower.
- Add a Glass Pane to a third side. Expect a new arm and a raised post. Remove the pane, then stack another wall with its post raised. Expect the lower post to rise with the upper one.

## Create stone palette walls (Pumpkin `101e2f325`)

- Craft Cut Andesite Walls and stonecut Small Veridium Brick Walls. Place them alongside Cobblestone Walls, Glass Panes and properly aligned fence gates. Expect each wall to keep its own material, connect across block types and block jumping like a vanilla wall.
- Cover a straight section with a bottom slab, then swap in a top slab. Expect tall arms with the bottom slab and low arms with the top slab. Use a water bucket on a wall and restart. Expect the same post and arms, water inside the wall, and matching geometry from either side.
- Assemble a Cut Calcite Brick Wall on a Mechanical Bearing and restart while it runs. Expect the wall to keep its material and arms through motion and reload. Change a neighbor at the landing point and stop the bearing. Expect the arms to update against the new neighbor.
- Build equal large arrays of Cobblestone Walls and Cut Calcite Walls. Pan across them and cross the chunk boundaries in both directions. Record frame timing, overlay pop-in, texture seams, and any exposed Cobblestone Wall surface, especially looking along a row from below or through water.


## Vanilla pane and bar connections (Pumpkin `3d233d7d8`)

- Connect a Glass Pane to Iron Bars and a Cobblestone Wall. Add and remove a block above the pane. Expect the horizontal connections to stay in place. Remove one horizontal neighbor and expect only that side to retract.
- Waterlog Waxed Copper Bars and connect them to a wall. Expect the water to remain when you add a block above or remove the wall. Restart and expect the same connections and water.

## Create window and glass panes ([issue #39](https://github.com/devmogged/moddedMC-rust/issues/39), Pumpkin `3d233d7d8`)

- Craft Oak Window Panes from six Oak Windows and stonecut Glass Panes into Framed Glass Panes. Expect 16 from the crafting recipe and one from the stonecutter. Mine with a Silk Touch tool to recover a pane.
- Place Framed Glass, Oak Window and Birch Window Panes between vanilla Glass Panes, Iron Bars and walls. Expect the arms to connect on the correct sides, recheck the ends after removing a neighbor, and compare the edges between matching panes. Look through Birch Window Panes at close range to check translucency.
- Place a pane into a water source and another next to Waxed Oxidized Copper Bars. Expect the pane to hold water and the stock bars to keep their copper appearance. Restart and expect the same geometry and water.
- Rotate an Oak Window Pane on a Mechanical Bearing. Restart while it runs, remove one neighbor at the landing position, and stop the bearing. Expect the moving pane to keep its material and update its arms on landing.
- Pan across large arrays of Framed Glass and Oak Window Panes, including across chunk boundaries. Record frame timing, overlay pop-in, edge seams, and any differences between the two rendering paths.

## Create metal bars ([issue #40](https://github.com/devmogged/moddedMC-rust/issues/40), engine `bb5eeda87`)

- Stonecut Andesite Alloy, a Brass Ingot and a Copper Ingot. Expect four of the corresponding bar per input. Place each alongside stock Iron Bars, Glass Panes and a wall. Expect copper placement sounds, a cap on the loose end of a single connection, and the correct shape at corners and junctions.
- Place Brass Bars in source water. Restart, then sneak-use a wrench in Survival. Expect one Brass Bars item in your inventory, water in the old position, and the neighboring arms to retract. Repeat in Creative and expect no recovered item.
- Rotate waterlogged Brass Bars on a Mechanical Bearing. Expect water to remain at the source during motion. Restart and stop the bearing beside a different neighbor. Expect the rendered bars to keep their material through motion and reconnect and waterlog at the landing site.

## Create copper shingles and tiles ([issue #41](https://github.com/devmogged/moddedMC-rust/issues/41), engine `fbf208e11`)

- Stonecut copper ingots into shingles or tiles. Expect two full blocks per ingot. Stonecut each block into stairs or slabs. Place two matching slabs together and mine the double slab with a stone pickaxe. Expect two slabs back.
- Place copper roofing near vanilla copper and wax one side. Expect the unwaxed roofing to age and the waxed roofing to keep its color. Use an axe twice on a waxed, waterlogged stair. Expect wax removal first, then one stage of scraping with the water and shape intact. Strike an exposed roof block with lightning and expect it to return to fresh copper.
- Mount a waxed, waterlogged tile stair on a Mechanical Bearing. Run the bearing, restart the server, then stop the bearing. Expect the stair to show its roof material in motion and reassemble with the waxed finish, shape and water intact.
- Build a roof with shingle blocks, slabs and corner stairs. Expect the released textures to load. Check and record seams on the upper faces. The stock-client pack currently uses fixed models for these faces.

## Create metal ladders ([issue #42](https://github.com/devmogged/moddedMC-rust/issues/42), engine `98618b955`)

Use the official 26.3 client with the Fabric companion and the server resource pack.

- Stonecut Andesite Alloy or a Brass or Copper Ingot. Expect two matching ladders. Attach one to a wall. Look down and use another ladder on it. Expect it to hang beneath the first even where the wall stops. Look up and extend the supported run. Expect each assisted placement to spend one ladder in Survival.
- Climb a run of each metal and press sneak while on it. Expect the same movement as on a stock ladder. Check the hit outline, orientation, hoop texture and whether touching ladders hide the right faces.
- Place a brass ladder into source water under another brass ladder. Restart and expect both ladders and the water to remain. Remove the upper ladder and expect the lower ladder to drop, leaving water behind.
- Place a ladder where a second shoulder wall can support it. Use a wrench from above and expect the ladder to turn onto that wall. Sneak-use the wrench in Survival and expect the ladder item back. Place stock waxed copper trapdoors beside the run and check that they retain their original appearance and remain unclimbable on their own.

## Create metal scaffolding ([issue #56](https://github.com/devmogged/moddedMC-rust/issues/56))

Create 6.0.10. Use Java 26.3 with the updated Fabric companion and accept the server pack.

- Stonecut Andesite Alloy, a Brass Ingot and a Copper Ingot. Expect two matching scaffolds from each. Place one against the side of a wall above air and extend it from the side. Expect a vertical stack that stays up when the wall is removed.
- Look along a matching row and use the top face. Expect the row to extend in the direction you face. Use a side while sneaking. Expect the row to extend toward that face, spending one block per placement. Fill seven positions in a row, click from the start, and expect the eighth position to remain empty.
- Jump into a column and sneak on a non-bottom segment. Expect to climb and descend. Walk onto the top from an adjacent block and expect to stand on the platform. Step into a bottom segment from below and expect its upper half to block movement. Check the outline with the matching scaffold in hand, then with an empty hand.
- Extend a row into source water. Restart and expect it to remain waterlogged. Sneak-use a wrench in Survival and expect the scaffold item back and the water to remain.
- Build a floating 3?3 platform of one metal and remove one corner. Expect connected top textures, the inner corner tile and hidden side seams. Place a second metal next to it and expect the colors to stay separate.
- Place stock bamboo scaffolding and waxed copper trapdoors beside all three variants. Expect bamboo to collapse normally when unsupported and stock trapdoors to retain their original look and behavior.

## Create stationary trapdoors ([issue #58](https://github.com/devmogged/moddedMC-rust/issues/58))

Use Java 26.3 with the Fabric companion and the server resource pack.

- Craft a Train Trapdoor and a Framed Glass Trapdoor. Place each on the upper and lower halves of a wall and on the top and bottom faces of a block. Expect each hinge, hit outline and opening direction to match the placement. Expect the Train Trapdoor to show its metal texture and the Framed Glass Trapdoor to let you see through it.
- Toggle each by hand, then power it and toggle it by hand again. Expect one movement and one iron trapdoor sound per toggle. Remove and restore power and expect the next redstone edge to set the corresponding open state. Throw a wind charge near an unpowered trapdoor and expect it to toggle. Repeat with a powered trapdoor and expect its state to stay put.
- Put two matching trapdoors side by side in the same half. Expect the touching faces to be hidden. Open them together and check the side seam. Put one of each material next to the other and expect their borders to stay visible.
- Place a Framed Glass Trapdoor in source water, open it, and restart the server. Expect the opening and water to persist. Sneak-use a wrench in Survival and expect the item back with the source water left behind.
- Place a stock Waxed Oxidized Copper Trapdoor nearby and open it. Expect vanilla copper textures and the original collision and sound.
