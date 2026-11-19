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

Raw JSON is at `run/harness-investigation/final.json`. Each reported directory contains the two console logs, worlds and any bot logs for every checkpoint.

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
