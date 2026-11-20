# Create kinetics

`create.mmc` adds the shaft, cogwheel, large cogwheel, creative motor, gearbox, clutch, gearshift, andesite encased shaft, andesite casing, turntable and mechanical belt, with Create's stress model. Its mechanics are ported from [Create 6.0.10](https://github.com/Creators-of-Create/Create/tree/mc1.21.1-6.0.10/src/main/java/com/simibubi/create/content/kinetics) (tag `mc1.21.1-6.0.10`, commit `ac0c444`): `src/kinetics.rs` follows `RotationPropagator`, `KineticNetwork`, `KineticBlockEntity` and `GeneratingKineticBlockEntity` method by method, so speeds, sources, networks, overstress and the block that breaks on a conflict match Create. The mod needs the ABI 1 content and block-world interfaces, including the block-world `has_neighbor_signal` and `destroy` extensions.

1. Build with `scripts/pack-mod.ps1 -Crate create`. Place `target/mods/create.mmc` in the server's `mods/`.
2. Supply your local Create jar through `MODDEDMC_ASSET_JARS`, a platform-separated list of paths. The host also looks in `cache/` and `D:/minecraft/buja_server/mods`. The generated pack and its SHA-1 stay in `cache/moddedmc/`.
3. Make the pack HTTP port reachable from the client. It defaults to the Java server port plus one. Use `MODDEDMC_PACK_BIND` to override the listener and `MODDEDMC_PACK_URL` to supply a reverse-proxy URL when players connect through a game tunnel.

Create code uses the [MIT license](LICENSE.md). The visual assets are reserved by the Create team and read at startup from the supplied jar. Recordings of the play-test below should show the running game.

## Stock 26.3 play test

1. Join a scratch server in Creative and accept the resource pack. Get the items with `/give @s create:creative_motor`, `/give @s create:shaft`, `/give @s create:large_cogwheel` and `/give @s create:cogwheel 2`. Check that all four icons use their Create models.
2. Place stone at `1025 99 1025`, `1026 101 1026` and `1027 101 1026`. Face straight down onto the first stone and place the motor facing up at `1025 100 1025`. Place the shaft on the motor and the large cog on the shaft. Place one small cog on each of the other stone blocks.
3. Look down at the gears. The large cog should turn counterclockwise at 16 RPM. The first small cog should turn clockwise at 32 RPM. The second small cog should turn counterclockwise at 32 RPM. Run `/kinetics 1026 102 1026` and expect `Speed=-32`.
4. Break the shaft. Both small cogs and the large cog should stop while the motor continues. Replace the shaft, run `/kinetics 1025 100 1025 24`, then restart the server. On rejoin the motor and large cog should turn at 24 RPM and the two small cogs at -48 and +48 RPM.
5. Capture one recording from above showing the gears reversing across the mesh, stopping on the shaft break, and resuming after restart.

The [bot scenario](../../tests/scenarios/p1a-create.txt) covers the pack download, item placement, RPMs, restart, and breaking the shaft.

Stress and transmitters (P1b):

1. Get `create:gearbox`, `create:clutch`, `create:gearshift`, `create:turntable` and a few `create:shaft` with `/give`.
2. Place a motor facing east at `2000 100 2000` and a gearbox east of it, then shafts on the gearbox's east, north and south faces. Expect the east and north shafts to turn opposite to the motor's shaft and the south shaft to turn with it (`/kinetics` reads `Speed=-16`, `-16` and `16`).
3. On a line motor, gearshift, shaft, clutch, shaft (all along X), put a redstone block on the gearshift. Every shaft past it reverses. Put a redstone block on the clutch: the last shaft stops while the clutch keeps turning. Remove that redstone block and the last shaft turns again.
4. Set a motor to 1 RPM, gear up eight times (large cog to small cog, `x2` each) to 256 RPM and put 17 turntables on meshed cogs at 256 RPM. Every part of that network, the motor included, stops (`/kinetics` shows `Stress=17408 Capacity=16384 Overstressed`). Break one turntable and everything turns again.
5. Run two motors at 16 RPM facing each other, each with one shaft, and place a third shaft between them. The new shaft breaks and drops, and both lines keep turning.
6. Restart, rejoin and check that steps 2 to 5 look the same.
7. Capture one recording of steps 3 and 4.

[`tests/scenarios/p1b-create.txt`](../../tests/scenarios/p1b-create.txt) checks all of this with the bot, and [`tests/bench/p1-create-shafts.txt`](../../tests/bench/p1-create-shafts.txt) measures a 256-shaft line.

Belts (P1c stage 1, `src/belt.rs` and the belt parts of `src/kinetics.rs`, from `BeltBlock`, `BeltBlockEntity` and `BeltConnectorItem`):

1. Craft a mechanical belt from six dried kelp, or `/give` yourself `create:belt_connector`.
2. Place a motor facing east at `2049 100 2010`, X-axis shafts at `2050 100 2010` and `2050 100 2014`, and one more at `2051 100 2014`. Click the first shaft, then the second, with the belt. A four-block belt appears between them and one belt item is used up in survival. Every segment and the far shaft turn at the motor's speed.
3. Try joining shafts that don't line up (different axes, or more than 20 blocks apart). Nothing is placed. Sneak-click to forget a started connection.
4. Break the middle of the belt. The whole belt goes, the two pulley segments become shafts again, and the far shaft stops.
5. Restart and check that the belt still turns.
6. Summon a mob on the running belt (`/summon minecraft:pig 2050.5 101 2011.5`). It slows down, centres on the belt and rides it towards the far shaft at 16/480 of a block a tick. Below 240 RPM, another entity in its path stops it.
7. Stand on the running belt yourself. It carries you the same way and stops pulling you to the middle while you hold a movement key. Sneaking or flying steps you off.
8. Throw a stack of iron ingots and one of cobblestone onto the running belt. Each lands, disappears as an item and rides the belt drawn at half size, the ingots lying flat and the cobblestone standing, a block apart. Past the far end they pop off as items again. Put a solid block just past the end and they wait there instead; put a second running belt there and they move onto it.
9. Throw a stack on the belt and right-click the segment it rides on with an empty hand. The stack goes into your inventory with the pickup sound.
10. Capture one recording of steps 2, 4, 6, 7, 8 and 9.

[`tests/scenarios/p1c-belt.txt`](../../tests/scenarios/p1c-belt.txt) checks steps 2 and 4 with the bot, the bot riding for step 7, an armor stand riding for step 6, an ingot stack picked up, drawn and ejected for step 8, and one taken by hand for step 9.

Players are carried differently from Create, which moves them on its own client: a stock client stands on the belt's full carrier block, so the server finds players resting on each moving belt and sends them the belt's push as motion. The bot simulates its own movement like a stock client, so the scenario checks this too; step 7 still covers how it feels. A sloped belt can't lift a player, because its carrier collides as a full block.

Items on belts (`src/belt_inventory.rs`, from `BeltInventory`, `TransportedItemStack` and `ItemHandlerBeltSegment`) are saved in the controller's `Inventory` like Create. They are drawn with item displays: the port can't tell which items Create draws upright (fluid containers and the `create:upright_on_belt` tag), so all of them lie flat or, for block items, stand; stacks aren't drawn as piles, and items don't tilt on slopes.

### Depot

Craft a Depot from an Andesite Casing and an Andesite Alloy. The Depot saves its `HeldItem` and eight-slot `OutputBuffer` in Create's NBT format. Items on its top and in the buffer are shown with item displays.

1. Place a Depot and right-click its top with a stack of iron ingots. The stack slides onto the top. Click a side with an empty hand and check that the stack stays there. Click the top with an empty hand and check that the stack returns to your inventory.
2. Drop an item onto the top. It should settle into the Depot. Break the Depot and check that the item drops.
3. Put a Depot immediately after a running belt. Send two stacks down the belt. The first moves onto the Depot and the second waits at the end. Take the first stack and watch the second slide onto the Depot.
4. Restart and check the second stack is still visible and can be taken. Capture the handoff and restart in one recording.

The [Depot bot scenario](../../tests/scenarios/create-depot.txt) checks placement, top and side use, falling items, breaking, belt handoff, and restart from the client's received block, entity and inventory packets. Analog output is deferred until content blocks can declare comparator support. [ABI requests](../../docs/abi-requests.md)

### Chute

1. Stack two chutes directly over a Depot and drop items into the top. The stack should appear on the Depot in order of arrival. Empty-hand use retrieves it.
2. Block the outlet with stone. Use a Wrench on the vertical chute to open its window. A stack should stop halfway down the chute and remain there until the outlet clears or you retrieve it with an empty hand.
3. Put one chute over a running belt and one at the outlet, with a Depot below the outlet chute. Watch a stack travel from the top chute onto the belt and reappear on the Depot.
4. Place a chute against the side of an existing chute. It should attach diagonally one level up and feed the chute you clicked. Capture the diagonal placement and the belt handoff.

The [Chute bot scenario](../../tests/scenarios/create-chute.txt) exercises these paths. Chute fan airflow, smart chutes, encasing and the original advancement effects are pending.

### Andesite funnels

1. Place an Andesite Funnel on the side of a container over a running belt. Sneak when placing it against an interactive container. It takes the belt-funnel shape and points away from the container.
2. Use a Wrench on a perpendicular belt funnel to switch between pushing and pulling. Feed a chest through a pushing funnel and watch the stacks enter the belt one item at a time. Put a pulling funnel beside a barrel farther along and check that the barrel collects the items.
3. Power the sending funnel with redstone. Transfers should pause and then resume when power is removed. On a normal upward collector, insert a stack by hand and check the container below it.
4. Feed a Depot from a blocked chute through a belt funnel. Capture the belt-to-barrel transfer and pause in a recording.

The [funnel bot scenario](../../tests/scenarios/create-funnel.txt) checks these transfers and the chest, barrel, furnace and hopper inventory paths. Brass filters, package handling and the animated flap are deferred. The transfer sounds play through the stock client.

### Mechanical Press

1. Place a Mechanical Press two blocks above a Depot. Feed power into either end of its shaft. Put two sugar cane on the Depot and watch each turn into paper on a separate stroke. Take both pieces of paper with an empty hand.
2. Run a belt under the press, with a Depot at its outlet. Drop an iron ingot onto the belt. It should stop under the press, then continue to the outlet as an Iron Sheet.
3. Put a full block below the press in place of the Depot. Drop sugar cane on it, let the item settle, and pick up the paper after the press runs.

The [press bot scenario](../../tests/scenarios/create-press.txt) checks all three inputs. Pressing recipes come from the merged datapack view, and tag ingredients resolve through the item tag registry on reload. The stock client sees the press housing and item displays and hears each stroke. The moving head and item particles are pending.

### Encased Fan

1. Power a fan with water in its airflow and run ice through it on a slow belt. Pick up Packed Ice downstream.
2. Blow through a regular lit campfire towards potatoes on a Depot, then through a lit soul campfire towards sand. Take the baked potatoes and soul sand. Putting out a campfire removes its catalyst.
3. Blow through lava onto raw iron on a Depot. Take the iron ingot. Save and stop partway through haunting a loose item. On restart the same item should finish with the remaining timer.

The [fan bot scenario](../../tests/scenarios/create-fan.txt) covers all four catalysts and the restart. Smoking and blasting require the base cooking recipes in the resources ABI. Fan particles, the spinning propeller and effects on living entities are pending.

### Millstone and Crushing Wheels

1. Power a Millstone from below or from a neighbouring cog. Drop two clay blocks onto it. Empty-hand use gives eight clay balls once both have milled. Stop it with clay still inside and use it again to recover the unprocessed input once the output slots are empty.
2. End a belt at the Millstone. Put a chute directly underneath, with a Depot below it. Send a clay block down the belt and take four clay balls from the Depot.
3. Place two Crushing Wheels on the same axis with one block between them. Drive them in opposite directions so the gap feeds downward, then put a Depot beneath it. Drop andesite into the gap for cobblestone, followed by gravel for sand and a chance of bonus drops. Break either wheel and check the gap.

The [Millstone scenario](../../tests/scenarios/create-millstone.txt) and [Crushing Wheels scenario](../../tests/scenarios/create-crusher.txt) cover these paths. The wheels use a stock model until the companion can draw Create's OBJ. Mechanical crafting, crushing damage to mobs and players, and the original processing sounds and particles are pending.

### Mechanical Bearing

Place a bearing with a slime block on its rotating face and attach cobblestone to the slime. Power the bearing at 15 RPM. Stop it after about one second and check that the cobblestone appears a quarter turn from its starting point. An empty-hand click disassembles a running bearing.

The [bearing scenario](../../tests/scenarios/create-bearing.txt) checks attachment, rotation and disassembly, including log and shaft axes and stair facings. Connect a motor to each rotated shaft to check that it still transmits power on its new axis. The scanner leaves an assembly in place when a neighbour needs unsupported handling and keeps a moving assembly intact when its destination is obstructed. Stock clients see the moving block displays.

Attach a single chest and a furnace to the same frame. Put distinct stacks in them, start the bearing and restart the server while it moves. After disassembly, open the two containers at their rotated positions and check the same stacks are inside. The [storage scenario](../../tests/scenarios/create-bearing-storage.txt) also checks for loose items at pickup and after disassembly. Double chests and waterlogged storage are pending.

Attach one pulley of a dry horizontal belt to the frame. Load an item, disconnect the belt's drive, and turn the bearing a quarter turn. The whole belt and its item should follow. After disassembly, take the same stack and connect a motor on the newly rotated pulley axis. The [moving belt scenario](../../tests/scenarios/create-moving-belt.txt) checks the stock display and restarts the server during the move. The [belt slope scenario](../../tests/scenarios/create-moving-belt-slopes.txt) checks that an ascending belt retains its grade and a vertical belt exchanges its end parts when yaw reverses its facing sign. Waterlogged belts and rotations about X or Z are pending.

Place a Windmill Bearing with a shaft on the back and eight wool blocks attached to a slime frame on the front. Empty-hand click the bearing to start it. The shaft should turn at 1 RPM with 512 SU available. Click again to stop it and put the blocks back. The [windmill scenario](../../tests/scenarios/create-windmill.txt) checks both clicks and the eight-sail minimum. Sail blocks and the rotation-direction control are pending.

### Mechanical Piston

Place a Sticky Mechanical Piston facing east above a Creative Motor facing up. Add three extension poles behind the piston, and attach a slime frame and platform at the front. Drive the motor at -128 RPM to extend three blocks. Reverse it to pull the platform back. Put obsidian in the platform's path and extend again. The platform should stop at the last clear block. Capture the extension, return and collision in one recording.

The [piston scenario](../../tests/scenarios/create-mechanical-piston.txt) checks placement, moving displays, all three steps of travel, and pole and head placement. The port uses the default movement mode and stops at loaded terrain collisions. Collisions between moving contraptions and actor block exceptions are pending.

Click the front of a retracted Mechanical Piston with a slimeball in Survival. It becomes sticky and consumes one slimeball. Try Adventure mode or click a side face and check that the stack stays intact. Extend a piston and break a front pole or the head. The front assembly comes off and the base retracts. Breaking the base also removes the rear poles. The [conversion](../../tests/scenarios/create-piston-sticky.txt) and [break](../../tests/scenarios/create-piston-break.txt) scenarios cover those paths.

### Gantry

Connect four matching Gantry Shafts to a motor and place a Gantry Carriage on the first shaft. Attach slime and a block above it. Apply redstone anywhere on the shaft line and turn on the motor. The carriage should remain in place and transfer rotation. Remove the redstone to travel to the far end, then reverse the motor to return. The [gantry scenario](../../tests/scenarios/create-gantry.txt) checks both modes, endpoint placement and the carriage breaking when its supporting shaft is removed.

The starting shaft holds the moving structure's save while its chunk is loaded. Sequenced gearshift limits, the shaft extension placement helper and collision with other moving contraptions are pending.

### Rope Pulley

Connect a Rope Pulley to a motor, lower its magnet to a slime platform, then reverse the motor to lift the platform. Drive it down again with a solid obstacle in the platform's path. The platform stops and the rope becomes blocks up to the pulley. The [rope pulley scenario](../../tests/scenarios/create-rope-pulley.txt) checks the cable displays, the stop above obsidian and the lift to the top.

The stock view draws the moving cable with two block displays. Synchronized pulleys, movement mode controls, and carrying players are pending.

### Chassis

Put a Linear Chassis on a bearing and right-click one axial face with a slimeball. Put a row of blocks against that face and turn the bearing. The chassis takes the row and leaves the blocks beside its unglued faces. A Radial Chassis takes the connected blocks in the plane perpendicular to its axis. The [chassis scenario](../../tests/scenarios/create-chassis.txt) checks both paths. A moving chassis carries its range and rotates its glued faces with the structure. The stock view uses the radial base model. Range scrolling and radial glue overlays are pending.

Put a Sticker on a bearing with a block touching its front. Pulse redstone to extend it, then turn the bearing. The front block moves and side blocks stay. Pulse it again to release the front block. The [sticker scenario](../../tests/scenarios/create-sticker.txt) checks both pulses and the rotated attachment. The moving head animation and upright bounce are pending.

### Riding contraptions

Put an armor stand and a loose item on a cobblestone platform attached to a piston. Extend the piston and check both stay on the platform. The [piston rider scenario](../../tests/scenarios/create-contraption-riders.txt) checks their positions after a three-block move. The [bearing rider scenario](../../tests/scenarios/create-bearing-rider.txt) checks that an armor stand follows a quarter turn around the vertical axis. The [tilted bearing scenario](../../tests/scenarios/create-bearing-tilted-rider.txt) checks that bearings lift stands when rotating about X and Z. Contraptions use their captured block shapes to push living entities and items. Player carrying, special block reactions and other entity types are pending.

Not ported yet: the rest of `BeltBlock.useItemOn` (dyes and water, the wrench and the connector on an existing belt, packages, a shaft turning a middle segment into a pulley, casings), and its `mayBuild` check; belt processing, tunnels and crushers; the belt surface doesn't scroll, only its top half is drawn, and sideways belts (between vertical shafts) are drawn turned the wrong way, because block models can't take the Z rotation `BeltRenderer` uses; breaking a pulley segment doesn't drop its shaft, belts can't be dyed or cased, and their placement makes no sound; the gearbox, clutch and gearshift show their casing without the turning shaft halves inside; the turntable spins mobs and items standing on it but not `setNoActionTime`, which Pumpkin lacks (players turn on their own client, as in Create); the vertical gearbox item, encasing a shaft with casing and casing recipes (Create's `item_application` recipe type); overstress particles and goggle tooltips; and the motor's in-world scroll control. The casing blocks use andesite as their physical template, so unlike Create they conduct redstone. The stock selection outline follows the vanilla carrier.
