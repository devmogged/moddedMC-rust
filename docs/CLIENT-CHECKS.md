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
