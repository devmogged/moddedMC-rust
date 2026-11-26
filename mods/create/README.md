# Create kinetics

`create.mmc` adds the shaft, cogwheel, large cogwheel, creative motor, gearbox, clutch, gearshift, andesite encased shaft, andesite casing, turntable and mechanical belt, with Create's stress model. Its mechanics are ported from [Create 6.0.10](https://github.com/Creators-of-Create/Create/tree/mc1.21.1-6.0.10/src/main/java/com/simibubi/create/content/kinetics) (tag `mc1.21.1-6.0.10`, commit `ac0c444`): `src/kinetics.rs` follows `RotationPropagator`, `KineticNetwork`, `KineticBlockEntity` and `GeneratingKineticBlockEntity` method by method, so speeds, sources, networks, overstress and the block that breaks on a conflict match Create. The mod needs the ABI 1 content and block-world interfaces, including the block-world `has_neighbor_signal` and `destroy` extensions.

1. Build with `scripts/pack-mod.ps1 -Crate create`. Place `target/mods/create.mmc` in the server's `mods/`.
2. Supply your local Create jar through `MODDEDMC_ASSET_JARS`, a platform-separated list of paths. The host also looks in `cache/` and `D:/minecraft/buja_server/mods`. The generated pack and its SHA-1 stay in `cache/moddedmc/`.
3. Make the pack HTTP port reachable from the client. It defaults to the Java server port plus one. Use `MODDEDMC_PACK_BIND` to override the listener and `MODDEDMC_PACK_URL` to supply a reverse-proxy URL when players connect through a game tunnel.

Create code uses the [MIT license](LICENSE.md). The visual assets are reserved by the Create team and read at startup from the supplied jar. Recordings of the play-test below should show the running game.

Use an andesite or brass casing on a shaft to cover it while keeping its axis and rotation. The casing stays in your hand. Sneak-use the wrench to uncover the shaft. The [shaft casing scenario](../../tests/scenarios/create-shaft-casing.txt) checks both materials, rotation across restart, and wrench removal.

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

### Water Wheel

Craft a Water Wheel with eight planks around a Shaft. Mount it on an axle with space for the paddles, then run water down one side. It drives the shaft at 8 RPM and provides 256 SU. Take the water away to stop it, or put the water on the other side to reverse rotation. The stock pack uses a spruce rotor built from vanilla textures.

The [Water Wheel scenario](../../tests/scenarios/create-water-wheel.txt) crafts and places the wheel in Survival, rejects an adjacent wheel in the paddle plane, checks speed and stress capacity, restarts, then shuts off and reverses the flow. In stock Java 26.3, check that the paddles follow the axle and turn with the water, and that the wheel item shows in the inventory.

### Hand Crank

Craft a Hand Crank with a row of three planks and an Andesite Alloy below the rightmost plank. Place the crank against a shaft. Use it to supply 32 RPM for ten ticks, or sneak-use it to reverse. Repeated use keeps it running and costs 0.32 food exhaustion per click. Breaking the support drops the crank.

The [Hand Crank scenario](../../tests/scenarios/create-hand-crank.txt) crafts it in Survival, checks both directions, checks exhaustion and restart, and removes its support. In stock Java 26.3, hold use and watch the handle and shaft turn together. Release to stop, then sneak-use to check the reverse direction.

### Large Water Wheel

Surround a small Water Wheel with eight planks in the crafting grid. Place the Large Water Wheel onto a shaft with a clear 3×3 plane perpendicular to the axle. Falling water two blocks out turns the wheel at 4 RPM and supplies 512 SU. Mining a corner removes the whole wheel and drops one item.

The [Large Water Wheel scenario](../../tests/scenarios/create-large-water-wheel.txt) checks crafting, blocked placement, axle extensions, flow, restart and mining across a chunk boundary. In stock Java 26.3, watch the three-block wheel and the shaft turn together, then mine an outside paddle and collect the single wheel.

### Windmill sails and direction

Craft white sails in a 2×2 square with wool and a stick on the top row, a stick and an Andesite Alloy below. Place eight sails or frames on a rotor and use its Windmill Bearing with an empty hand to turn it at 1 RPM and 512 SU. Use shears on a white sail to expose its frame. A Wrench click on the bearing reverses direction and displays the setting in the action bar. The direction saves with the bearing.

The [windmill sails scenario](../../tests/scenarios/create-windmill-sails.txt) checks player crafting, placing a sail alongside another, shearing, reversal, restart and disassembly. In stock Java 26.3, check that the sails look the same while placed and while rotating, and that the sail assembly and output shaft reverse together.

### Mechanical Crafter

Craft three Mechanical Crafters by stacking an Electron Tube, Brass Casing and Crafting Table in a crafting grid. Mount the crafters in one plane, power them with a neighbouring Cogwheel, and click their fronts with a Wrench to route each arrow toward one output. Use each front to load one ingredient. A Crafter Slot Cover marks an empty recipe cell, and an empty hand retrieves an ingredient or cover before the machine starts.

The [Mechanical Crafter scenario](../../tests/scenarios/create-mechanical-crafter.txt) makes two Crushing Wheels from the released 5×5 recipe with an output barrel. It restarts while ingredients are in transit and checks the final count. On stock Java 26.3, check that the arrows turn with each Wrench click, the front shows the loaded ingredient or cover, and the barrel receives two Crushing Wheels. Crafters also use ordinary crafting recipes, including loaded datapack recipes, and send container remainders through the output. The [ordinary recipe scenario](../../tests/scenarios/create-crafter-ordinary-recipes.txt) checks one cake and three empty buckets across restart, followed by eight shafts.

Use a Wrench on the back seam between two crafters with the same orientation to share their input. After linking, one Andesite Funnel, Chute, Hopper or Dropper can fill the group's open slots in order, starting at the top left as seen from the front. Click the seam again to separate the inputs. The [connected-input scenario](../../tests/scenarios/create-crafter-connected-input.txt) checks linking, restart, insertion through one funnel, crafting and unlinking. The [hopper and dropper scenario](../../tests/scenarios/create-crafter-hopper-input.txt) also checks that a full group leaves the extra items in the sender.

### Rotation Speed Controller

Craft a Rotation Speed Controller with a Precision Mechanism above a Brass Casing. Use a Large Cogwheel on the controller to mount it on top across the controller's shaft axis. Power the cogwheel and connect a shaft to either end of the controller. The shaft starts at 16 RPM. On stock Java 26.3, click the left or right half of a side plate with an empty hand to lower or raise the speed by 1 RPM. Sneak-click to adjust by 16. The action bar shows the selected RPM.

The [controller scenario](../../tests/scenarios/create-speed-controller.txt) checks Survival crafting, blocked cog placement, cog consumption, 15, 16, 0 and -16 RPM output from a -32 RPM input, the saved -16 RPM setting, and driving the cog from a powered side shaft. On stock Java 26.3, check that both side plates are clickable, the shaft stops at 0 and reverses at -16 while the cog keeps turning, and the output still turns backwards after reconnecting to the restarted server.

### Encased Chain Drive and Adjustable Chain Gearshift

Craft an Encased Chain Drive from an Andesite Casing and three iron or zinc nuggets. Add an Electron Tube to craft an Adjustable Chain Gearshift. Place the casings next to one another across their shaft axes. Their end caps join into a straight chain that transmits rotation sideways. A Wrench turns the axle, and sneak-use removes the casing into your inventory.

Power the Adjustable Chain Gearshift through its shaft and send redstone into its casing. The chain receives the same speed at signal 0, 1.5 times the speed at signal 7, and twice the speed at signal 15. Power the chain from the other end to get the inverse ratio on the Gearshift's shaft.

The [chain drive scenario](../../tests/scenarios/create-chain-drive.txt) checks Survival crafting, the three speeds, a restart with signal 7, a repeater aimed into the adjacent casing, and Wrench rotation and pickup. In stock Java 26.3, check that all three casings look like a connected chain, that the Gearshift changes color and the far axle speeds up under redstone, and that removing the middle block leaves two separate end blocks.

### Brass Funnel

Stack an Electron Tube, Brass Ingot and Dried Kelp in the crafting grid to make two Brass Funnels. Sneak-place one against a container to collect items. Use a Wrench to switch to extraction. Over a running belt it becomes a Brass Belt Funnel. Power the funnel with redstone to pause it.

Right-click the filter plate with a sample item to show it on the funnel and allow only that item type. The sample stays in your hand. For an extractor or belt funnel, click the plate with an empty hand to set its batch size. The middle switches between **Up to** and **Exactly**. Click just left or right of the middle to adjust by one, or near the edges to adjust by 16. Sneak-click with an empty hand to clear the filter. The action bar shows the current item and setting.

The [Brass Funnel scenario](../../tests/scenarios/create-brass-funnel.txt) crafts two funnels in Survival and sets the extractor to exactly three iron ingots. It waits with only two in stock, then moves three after restart and leaves four gold ingots in the source. It also checks that breaking the belt preserves the filter and that a powered upward funnel rejects insertion. In stock Java 26.3, check that the sample sits on the filter plate, click each control and read the action bar, and watch a batch travel down the belt into the receiving funnel.

### Smart Chute

Stack a Brass Sheet, Chute and Electron Tube to craft a Smart Chute. Mount it vertically between a source inventory and an outlet. Apply a sample to a side plate to select the item type. The stock client uses the same empty-hand plate controls as the Brass Funnel when the chute is over an outlet. Redstone closes the chute, including when a batch is already inside it.

The [Smart Chute scenario](../../tests/scenarios/create-smart-chute.txt) crafts the chute, waits for exactly three iron ingots, and restarts with the batch held under redstone. Removing power releases the three ingots and leaves the gold behind. In stock Java 26.3, check that the sample is visible on the side you clicked, that the chute changes appearance under redstone, and that reopening it releases one batch after restart.

### List Filter

Put wool between two iron nuggets in a crafting grid to make a List Filter. Hold it in the main hand and right-click in the air with a sample in the offhand to add or remove that sample. The sample stays in the offhand. Sneak-use with a sample to toggle component matching, such as requiring the same tool damage. Sneak-use with the offhand empty to switch between an allow list and a deny list. Use with both the offhand empty and sneak released to read the current mode and a summary of the samples. Craft the filter by itself to reset it.

Click the filter plate on a Brass Funnel or Smart Chute with the configured List Filter to install it. Sneak-click the plate with empty hands to take it back. A replaced filter goes back into your inventory, and breaking the machine drops its installed filter with its settings intact.

The [List Filter scenario](../../tests/scenarios/create-list-filter.txt) crafts and edits a filter, installs it in a Smart Chute, and verifies both modes, exact component matching, and recovery after restart. In stock Java 26.3, check the action bar while editing the list, the installed filter on the side plate, and that a recovered filter shows the same settings.

### Attribute Filter

Craft a Brass Ingot into nine Brass Nuggets. Put wool between two Brass Nuggets to craft an Attribute Filter. Hold the filter in your main hand and a sample in your offhand. Sneak-use in the air to cycle through the sample's item tags, mod source and properties. Release sneak and use again to add the selected rule. Repeat to invert it, then repeat again to remove it. With an empty offhand, sneak-use to cycle between **Allow any**, **Allow all**, and **Deny any**. Use without sneaking to read the current rules.

Install and recover it on a filter plate as you would a List Filter. The [Attribute Filter scenario](../../tests/scenarios/create-attribute-filter.txt) applies coal's tag to charcoal and matches a second item by its source, including after restart. In stock Java 26.3, cycle the coal sample between its tags and confirm the action bar shows the selected rule, then install it and try a coal, charcoal and gold mix.

Coal also offers `furnace_fuel`, which matches sticks. Wet sponge offers `smeltable`, which matches raw ore. Worn tools offer `damaged`. Check these three rules on one filter by sending sticks, raw iron, damaged shears and fresh shears through a chute in **Allow any** mode. The fresh shears should stay in the source. The [property scenario](../../tests/scenarios/create-attribute-filter-properties.txt) checks this through a restart.

Enchanted books can select a specific enchantment, including when the enchantment is on a tool. Written books can select their author or copy generation. Shulker Boxes offer their current fill level. Check that an Efficiency book's rule accepts a tool with Efficiency and that a partly filled shulker's rule accepts a partly filled box of another color. The [contents scenario](../../tests/scenarios/create-attribute-filter-contents.txt) also checks book authors and copies after restart.

A sample renamed at an anvil offers its exact custom name. A dyed item or firework offers its color, including a firework's fade color. Try a red rule with wool, dye and a firework, then use two different items renamed to the same text to check name matching. The [name and color scenario](../../tests/scenarios/create-attribute-filter-style.txt) checks both rules after restart.

Gear with an applied enchantment at its maximum level offers `max_enchanted`. Buckets offer `fluid_container` and their fluid. Select the water rule and send water and lava buckets through a chute. Only the water bucket should transfer. The [max enchantment and fluid scenario](../../tests/scenarios/create-attribute-filter-max-and-fluid.txt) checks these rules across restart with Efficiency IV, Efficiency V and Mending.

### Belt casings

Right-click a Mechanical Belt with an Andesite or Brass Casing to change its material. The casing item stays in your hand. A Wrench removes the casing. A solid block above a horizontal cased belt adds a matching cover.

On stock Java 26.3, apply both materials to a running belt and check that the bodies sit under the belt surface. Try a sloped segment and check that its casing follows the slope. Place and remove a block above a horizontal segment, then restart with the cover in place. Both materials and the cover should come back. The [belt casing scenario](../../tests/scenarios/create-belt-casing.txt) checks the right-click, wrench, covers, speed, and restart.

### Andesite Tunnel

Craft two tunnels from two Andesite Alloy above two Dried Kelp. Place a tunnel on a horizontal belt to add Andesite Casing wherever the belt is bare. Three tunnels in a row give the middle one a window. Use a Wrench to close or reopen it.

Put a Depot on each side of the tunnel at belt height and send five ingots through. Each side gets one and the main belt carries three. A full side outlet stalls the incoming stack. Stop the belt to clear both outlets, then power it again to release the stack. A Hopper or Dropper feeding the tunnel sends items into the belt below. Removing the supporting belt drops the tunnel.

The [Andesite Tunnel scenario](../../tests/scenarios/create-andesite-tunnel.txt) checks crafting, support, automatic casing, wrenching, dropper input, a three-way split and a jam cleared after restart. In stock Java 26.3, check that the junction openings face the side outputs and the window changes when wrenched.

### Brass Tunnel

Craft two with an Electron Tube in the top left of a crafting grid, two Brass Ingots below it, and two Dried Kelp in the last row. A Brass Tunnel cases a bare horizontal belt in brass. Adjacent tunnels on parallel belts share their outputs and mode. Put a sample item, List Filter or Attribute Filter on the upper center of an open side to filter that mouth. Matching filtered outputs are tried before unfiltered ones. Sneak-use with an empty hand to clear the filter.

Use a Wrench on top to cycle Split, Forced Split, Round Robin, Forced Round Robin, Prefer Nearest, Randomize and Synchronize. Sneak-use the Wrench to cycle backwards. The action bar shows the selected mode. Forced Split waits for its outlets to be available. Synchronize holds each parallel input until the whole group has a stack.

In stock Java 26.3, put two tunnels beside each other over parallel belts and watch Round Robin alternate between the two outputs. Install a filter and check that the sample sits on the mouth you clicked. Stop the server while Forced Split is blocked, restart, then empty the blocked output and watch the held stack leave. The [split and filter scenario](../../tests/scenarios/create-brass-tunnel.txt) and [connected group scenario](../../tests/scenarios/create-brass-tunnel-group.txt) cover these paths.

### Mechanical Arm

Hold the Mechanical Arm and right-click a depot, belt segment without a tunnel, chute or processing block to mark a pickup. Click again to change it to a drop-off. A deposit-only point starts as a drop-off. Sneak-use a selected point to remove it. Place the arm within five blocks of its points and drive it with a vertical-axis cog beside its base. The arm checks that a destination has room before taking a stack. Redstone pauses new pickups. Use a Wrench to cycle Round Robin, Forced Round Robin and Prefer First.

The [Mechanical Arm scenario](../../tests/scenarios/create-mechanical-arm.txt) configures two depots in Survival, holds a source under redstone, and restarts with four ingots in the claw. In stock Java 26.3, check that the selection status appears, the joints move together and the stack stays inside the claw while it swings. Try a ceiling mount and check that the arms and gear face down.

Use a Blaze Burner as a drop-off for fuel. The arm waits until the current burn is low and inserts one item per trip. Feeding a single lava bucket leaves the empty bucket in the claw. Click the arm to collect it. The [burner scenario](../../tests/scenarios/create-arm-blaze-burner.txt) checks the coal backlog across restart and the bucket return.

The vanilla special-purpose endpoints and movement of a configured arm on a contraption still need gameplay checks and ports.

### Threshold Switch

Craft it with an Electron Tube above a Brass Casing above a Comparator. Place it next to a chest, barrel or Fluid Tank. It points at the stock and emits redstone on the other sides. Click with an empty hand to read the total and cutoffs. The default output turns on at 128 and off at 64. Tanks are counted in millibuckets. A filter sample on the center of the outward face limits what counts. Sneak-use that face with an empty hand to clear the sample.

Use a Wrench on the outward face. The upper corners adjust the on cutoff and the lower corners adjust the off cutoff. Left decreases, right increases, and sneak-use adjusts by 64 items or 1,000 mB. The center inverts the output. Use the Wrench on another face to rotate the detector.

In stock Java 26.3, check that the indicator follows the stock level, the sample is visible on the outward face, and the action bar reports the two cutoffs. The [Threshold Switch scenario](../../tests/scenarios/create-threshold-switch.txt) verifies survival crafting, both lamp transitions, the latched state across restart, inversion and water filtering.

### Smart Observer

Craft it with an Electron Tube above a Brass Casing above a vanilla Observer. Place it beside an inventory, Depot, belt, funnel, pipe or tank. It points toward the target and sends redstone out the other sides when it detects matching contents or a transfer. Right-click the center of the outward filter plate with a sample to filter the signal. A filled bucket selects its fluid. Sneak-use with an empty hand to clear the sample, or use a Wrench to turn the observer. A block sample can also detect the block itself.

In stock Java 26.3, aim it at a barrel and connect a lamp to the outward side. Set an iron ingot as the sample and check that it appears on the filter plate. Add copper, then iron. Watch the lamp and observer shell change together when iron is added and return to their unpowered appearance when it is removed. Try a filter on a passing belt stack to see a brief pulse. The [stock and restart scenario](../../tests/scenarios/create-content-observer.txt) and [transport scenario](../../tests/scenarios/create-content-observer-transport.txt) check those signals, funnel transfers and fluid filtering.

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

The [Chute bot scenario](../../tests/scenarios/create-chute.txt) exercises these paths. Chute fan airflow, encasing and the original advancement effects are pending.

### Andesite funnels

1. Place an Andesite Funnel on the side of a container over a running belt. Sneak when placing it against an interactive container. It takes the belt-funnel shape and points away from the container.
2. Use a Wrench on a perpendicular belt funnel to switch between pushing and pulling. Feed a chest through a pushing funnel and watch the stacks enter the belt one item at a time. Put a pulling funnel beside a barrel farther along and check that the barrel collects the items.
3. Power the sending funnel with redstone. Transfers should pause and then resume when power is removed. On a normal upward collector, insert a stack by hand and check the container below it.
4. Feed a Depot from a blocked chute through a belt funnel. Capture the belt-to-barrel transfer and pause in a recording.

The [funnel bot scenario](../../tests/scenarios/create-funnel.txt) checks these transfers and the chest, barrel, furnace and hopper inventory paths. Package handling and the animated flap are pending. The transfer sounds play through the stock client.

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

Attach one pulley of a horizontal belt to the frame. Load an item, disconnect the belt's drive, and turn the bearing a quarter turn. The whole belt and its item should follow. After disassembly, take the same stack and connect a motor on the newly rotated pulley axis. The [moving belt scenario](../../tests/scenarios/create-moving-belt.txt) checks the stock display and restarts the server during the move. The [belt slope scenario](../../tests/scenarios/create-moving-belt-slopes.txt) checks that an ascending belt retains its grade and a vertical belt exchanges its end parts when yaw reverses its facing sign. The [tilted belt scenario](../../tests/scenarios/create-moving-belt-tilted.txt) checks that turning a loaded belt vertical ejects its stack at disassembly. The scanner leaves a diagonal belt in place when the bearing rotates about its run axis because that rotation can disconnect the chain. The [waterlogged belt scenario](../../tests/scenarios/create-moving-belt-water.txt) checks that pickup leaves source water behind and each segment samples the destination fluid when restored.

Place a Windmill Bearing with a shaft on the back and eight wool blocks attached to a slime frame on the front. Empty-hand click the bearing to start it. The shaft should turn at 1 RPM with 512 SU available. Click again to stop it and put the blocks back. The [windmill scenario](../../tests/scenarios/create-windmill.txt) checks both clicks and the eight-sail minimum. Sail blocks and the rotation-direction control are pending.

Place an upward Clockwork Bearing on a motor and attach separate block frames at one and two blocks above the bearing. Stop the daylight cycle and set the time to 03:15. With the motor at 15 RPM, both hands should rotate to the quarter-turn position. Restart while it runs, then stop the motor and check both frames return to their original blocks. The [clockwork scenario](../../tests/scenarios/create-clockwork.txt) checks both angles and the restart. The in-world mode selector and smooth rendering are pending.

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

Put an armor stand and a loose item on a cobblestone platform attached to a piston. Extend the piston and check both stay on the platform. The [piston rider scenario](../../tests/scenarios/create-contraption-riders.txt) checks their positions after a three-block move. The [bearing rider scenario](../../tests/scenarios/create-bearing-rider.txt) checks that an armor stand follows a quarter turn around the vertical axis. The [tilted bearing scenario](../../tests/scenarios/create-bearing-tilted-rider.txt) checks that bearings lift stands when rotating about X and Z. Contraptions use their captured block shapes to push living entities and items. Put an empty boat and minecart on a platform and extend the piston. Both should move with the platform and keep the new position across a restart, as checked in the [vehicle scenario](../../tests/scenarios/create-contraption-vehicles.txt). Stock-player carrying and special block reactions are pending.

Not ported yet: the rest of `BeltBlock.useItemOn` (dyes and water, the wrench and the connector on an existing belt, packages, a shaft turning a middle segment into a pulley, casings), and its `mayBuild` check; belt processing, tunnels and crushers; the belt surface doesn't scroll, only its top half is drawn, and sideways belts (between vertical shafts) are drawn turned the wrong way, because block models can't take the Z rotation `BeltRenderer` uses; breaking a pulley segment doesn't drop its shaft, belts can't be dyed or cased, and their placement makes no sound; the gearbox, clutch and gearshift show their casing without the turning shaft halves inside; the turntable spins mobs and items standing on it but not `setNoActionTime`, which Pumpkin lacks (players turn on their own client, as in Create); the vertical gearbox item, tool-based and multi-output item application recipes; overstress particles and goggle tooltips; and the motor's in-world scroll control. The casing blocks use andesite as their physical template, so unlike Create they conduct redstone. The stock selection outline follows the vanilla carrier.

### Elevator pulley

Drive an Elevator Pulley from its horizontal shaft. Place a movable platform below it with one
horizontal Redstone Contact on the platform. Place matching contacts facing it on the floors.
Use the pulley with an empty hand to assemble. Power a floor contact to call the platform and
check that its adjacent lamp lights when the platform arrives. Use the pulley again, or a pulley
mirroring it, to restore the blocks at that floor. The [elevator scenario](../../tests/scenarios/create-elevator.txt)
restarts the server mid-trip, checks both floor lamps and the mirror cable, then disassembles
through the mirror. Floor naming screens and contraption controls require client support.

### Super Glue

With Super Glue in hand, use two connected blocks to select opposite corners. Sneak and use
a block to clear the selection. A completed Survival selection uses one of the item's 99 charges.
Power a bearing or another actuator holding one of the glued blocks and check that the selection
moves together. The [Super Glue scenario](../../tests/scenarios/create-super-glue.txt) checks
the charge, restart, rotation, and the adjacent block that stays behind. The Create client
outline, click-to-remove overlay, and offhand placement shortcut still need support.

### Seats

Right-click a seat to sit, and use a different dye to recolor it. Assemble an occupied seat
onto a moving contraption and check that the rider stays aboard through disassembly. The
[seat scenario](../../tests/scenarios/create-seat.txt) checks a bearing turn, dismount on break,
dyeing and a pig that stays seated after restart. The stock view uses an invisible vanilla carrier. Mounting an
already moving seat and handing it a leashed mob need additional interaction hooks.

### Moving drills

Drive a stationary Mechanical Drill from its rear shaft. It mines the block in front of the drill and drops that block's loot. Faster rotation mines faster in either direction, and stopping the shaft pauses progress. Place a hopper below the target block to collect the output from a cobblestone generator. The [stationary drill scenario](../../tests/scenarios/create-drill-stationary.txt) checks block drops, cobblestone collection, and a restart during mining.

In stock Java 26.3, place the drill against a powered shaft and check that the head spins while the casing stays still. Place stone in front of the drill and check for a cobblestone drop. Stop the shaft partway through a harder block, restart the server, and reconnect power to finish mining.

A powered Drill also hurts players and mobs that touch its head. In a stock client, walk up to the head with the shaft stopped, then power it and approach again. Check that you take damage while touching the spinning head and that dropped items survive the contact. The [contact scenario](../../tests/scenarios/create-drill-contact.txt) checks the head and outer margin of the hitbox after restarting with power connected.

Mount a Mechanical Drill on a piston platform with the drill pointing forward. Put oak planks
in front of the head and extend the piston. The platform pauses while the drill mines, then
continues once the block drops. The [drill scenario](../../tests/scenarios/create-drill.txt)
checks mining progress, resumes across a restart, and checks the dropped item and final position.
Stationary mining, damage, falling-block stacks and break overlays are pending.


### Mechanical Saw

Craft the saw with three Iron Sheets around an Iron Ingot and an Andesite Casing below. Drive a horizontal saw from the rear. It cuts logs, leaves, and supported plant types. It checks the base of the remaining trunk before felling the tree, so a neighboring log can keep a multi-block trunk standing. Mining progress survives a restart.

In stock Java 26.3, mount the saw horizontally against a shaft, then start, reverse, and stop it. Check that the blade points at the log and changes appearance with the shaft. Cut into a broad trunk and confirm the upper trunk stays up until its supports are removed. The [tree scenario](../../tests/scenarios/create-saw-tree.txt) crafts and places the saw in Survival and checks those cuts and a restart.

### Mechanical Harvester

Attach a Mechanical Harvester to the front of a moving frame and run it down a crop row.
Mature wheat is cut and replanted, and the blade leaves immature wheat intact. Mount a chest
on the frame to collect the harvest. The [harvester scenario](../../tests/scenarios/create-harvester.txt)
checks both crop ages, the existing stack in the chest, and the saved crop and chest after restart.
