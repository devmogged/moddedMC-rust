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
6. Capture one recording of steps 2 and 4.

[`tests/scenarios/p1c-belt.txt`](../../tests/scenarios/p1c-belt.txt) checks steps 2 and 4 with the bot.

Not ported yet: belts don't carry items or entities yet, the belt surface doesn't scroll, only its top half is drawn, and sideways belts (between vertical shafts) are drawn turned the wrong way, because block models can't take the Z rotation `BeltRenderer` uses; breaking a pulley segment doesn't drop its shaft, belts can't be dyed or cased, and their placement makes no sound; the gearbox, clutch and gearshift show their casing without the turning shaft halves inside; the turntable spins mobs and items standing on it but not `setNoActionTime`, which Pumpkin lacks (players turn on their own client, as in Create); the vertical gearbox item, encasing a shaft with casing and casing recipes (Create's `item_application` recipe type); overstress particles and goggle tooltips; and the motor's in-world scroll control. The casing blocks use andesite as their physical template, so unlike Create they conduct redstone. The stock selection outline follows the vanilla carrier.
