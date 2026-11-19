# F2 harness baseline

Run on Windows against the official Java 26.3 server and a scratch copy of the supplied `pack-dev.exe`. Java was 26.0.1. The run used two parallel workers and returned exit 1 with **11 differences, zero allows and zero run errors**.

| Input | SHA-256 |
| --- | --- |
| `server-bundler.jar` | `d052f14d7a173734fba553711e5b570162e2f2a313267ee31a21b975a679be64` |
| `pack-dev.exe` | `11f5647874c3aeec855bed8d45e184f0f67a3a70caf677eb3dec02198398baea` |
| `moddedmc-bot.exe` | `64cb8a1184a4918c6ed0d6e0dbadae59e1f061d0bac6e2108dfeffa8a6a6123b` |

The exact command and preserved evidence are local to this run.

```powershell
run/harness-investigation/binaries/moddedmc-harness.exe run-all tests/harness --reference vanilla:D:/mc-ref/26.3/server-bundler.jar --subject run/harness-investigation/binaries/pack-dev.exe --bot run/harness-investigation/binaries/moddedmc-bot.exe --jobs 2 --run-root run/harness-investigation/final --json
```

Raw JSON is at `run/harness-investigation/final.json`. That run used temporary executables, worlds and logs. The JSON was retained and the temporary files were removed.

## Results

| Scenario | Differences | Observation |
| --- | ---: | --- |
| `basic.txt` | 2 | Fill, stair state and block probe matched. Barrel NBT differed. |
| `bot-place-break.txt` | 2 | Both servers acknowledged placement and break. The glass and air probes passed on both. Inventory NBT differed. |
| `piston-push.txt` | 6 | Moving piston NBT differed. The settled push matched. |
| `repeater-chain.txt` | 1 | The first repeater powered later on the subject. The settled snapshot matched. |
| `water-flow.txt` | 0 | Both water snapshots matched. |

## Every mismatch

Each comparison is reference versus subject. `<missing>` means the tag was absent in saved NBT.

### basic

```text
snapshot built @ 3 151 3: block_entity.components {} vs <missing> (line 9)
snapshot built @ 3 151 3: block_entity.keepPacked 0b vs <missing> (line 9)
```

### bot-place-break

```text
snapshot placed: player.alice.Inventory[0].components <missing> vs {} (line 14)
snapshot broken: player.alice.Inventory[0].components <missing> vs {} (line 18)
```

### piston-push

```text
snapshot moving @ 2 151 1: block_entity.blockState {id:"minecraft:piston_head",properties:{facing:"east",short:"false",type:"normal"}} vs <missing> (line 10)
snapshot moving @ 2 151 1: block_entity.components {} vs <missing> (line 10)
snapshot moving @ 2 151 1: block_entity.keepPacked 0b vs <missing> (line 10)
snapshot moving @ 3 151 1: block_entity.blockState "minecraft:stone" vs <missing> (line 10)
snapshot moving @ 3 151 1: block_entity.components {} vs <missing> (line 10)
snapshot moving @ 3 151 1: block_entity.keepPacked 0b vs <missing> (line 10)
```

### repeater-chain

```text
snapshot first-delay @ 1 151 1: block minecraft:repeater[delay=2,facing=west,locked=false,powered=true] vs minecraft:repeater[delay=2,facing=west,locked=false,powered=false] (line 12)
```

## Controls

- Same vanilla JAR on both sides, with the same seed, area, commands and tick steps: **zero differences**.
- Same vanilla JAR on both sides with one deliberate subject mutation after the tick step: exactly `snapshot planted @ 0 150 0: block minecraft:stone vs minecraft:dirt (line 6)`. The self-test passed both controls.
- A separate vanilla-vs-vanilla run matched two armour stands of the same type and position with different tags. Their per-run UUIDs did not affect the comparison.
- A nonexistent bot executable produced a line 7 launch error and exited 2. The error could not become a parity pass.

## Packet checkpoints

Run `client-effects.txt` with the same commands above using the current harness, bot and `pack-dev` executables from `engine/target/pack`. The fixture preloads the test area and clears generated chest minecarts before the measured window. Standalone lighting and terrain packets remain part of the comparison. Earlier calibration runs showed these packets can arrive late or in a different order in a fresh world.

- Vanilla 26.3 against the same jar exited 0 with zero differences. Both bots received the armor stand spawn, metadata and motion, the flame particle, and the sound. See `run/harness-packets/vanilla-checked.json`.
- Vanilla 26.3 against the pack at engine pin `68296347e` exited 1 with 42 differences and zero run errors. Five are packet comparisons covering the spawn follow-up order, entity metadata, particle flags and sound encoding. Two are player equipment tags and 35 are armor stand NBT. See `run/harness-packets/pumpkin-checked.json` for the individual values.
- The snapshot self-test matched identical vanilla worlds and caught the deliberate stone-versus-dirt change. See `run/harness-packets/self-test.json`.
