# Create kinetics

`create.mmc` adds a shaft, a cogwheel, a large cogwheel and a creative motor. Its mechanics follow the [Create mc1.21.1/dev source](https://github.com/Creators-of-Create/Create/tree/37ea4afcfe5bf85875d4b0749798a45330070a1f/src/main/java/com/simibubi/create/content/kinetics). The mod loads with the ABI 1 content and block-world interfaces.

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

The [bot scenario](../../tests/scenarios/p1a-create.txt) covers the pack download, item placement, RPMs, restart, and breaking the shaft. Java Create's rotation conflict and overspeed conditions destroy a block. This slice stops the conflicting component and retains its parts. Placement helpers, encasing, powered shafts, belts, stress and the motor's in-world scroll control are later work. The stock selection outline follows the vanilla carrier.
