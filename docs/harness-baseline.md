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

## Synchronized bot actions

Run `tests/harness/bot-actions.txt` against the same server pair. It checks the requested move and look through a position and rotation selector, attacks a pig, then moves three stone from hotbar slot 0 to slot 1 with two left clicks.

- Vanilla against itself exited 0 with no differences. The runner kept `report.json` and removed its checkpoints. See `run/harness-actions/vanilla-checked.json`.
- Both servers passed the position and rotation selector and reported pig health of 9. The pack run exited 1 with 36 differences and no run errors. It retained a checkpoint and a report. The differences include two probe outputs, two player NBT fields and 32 pig NBT fields. The pack rendered health as `9f` and rejected the inventory `execute if items` probe. Both saved the stone in the same slot. See `run/harness-actions/pumpkin-checked.json`.

## Cross-version oracles

The Windows reference loaded Create 6.0.10 on NeoForge 21.1.219 and Minecraft 1.21.1 with Java 21. The subject remained the 26.3 pack at engine pin `68296347e`. The reference installation is shared across runs. [Create 6.0.10 for NeoForge](https://modrinth.com/mod/create/version/UjX6dr61)

- `tests/harness-oracle/oak-boat.txt` translated `minecraft:oak_boat` to `minecraft:boat` for the reference. Both servers completed one exact tick and passed the probe. The run kept its JSON and removed its checkpoints.
- `tests/harness-oracle/create-shaft.txt` read `Speed: 0.0f` from the original mod. The base pack rejected the block ID and found no block entity. `run-all` returned 1 with two differences and zero run errors. The full results are in `run/harness-oracle/final.json`.
- The stock self-test still matched identical vanilla worlds and caught one deliberate block mismatch. See `run/harness-oracle/stock-self-test.json`.

## Linux verification

The native harness and bot passed Clippy and all nine unit tests on Ubuntu 24.04 under WSL2 with Rust 1.100.0. The pack build used `--target x86_64-unknown-linux-gnu --target-dir engine/target --profile pack --config profile.pack.incremental=false`. Runtime checks used Linux Temurin 26.0.2.1 for vanilla and Temurin 21.0.13.1 for NeoForge.

- The identical stock self-test matched, and the deliberate block change produced precisely one mismatch. See `run/harness-linux/self-test.json`.
- The action and packet scenarios matched between two native 26.3 vanilla instances. See `run/harness-linux/bot-actions.json` and `run/harness-linux/client-effects.json`.
- The NeoForge reference loaded Create 6.0.10. The mapped boat matched and the shaft probe returned `0.0f` against two expected differences from the vanilla subject. See `run/harness-linux/oracle.json`.

The Pumpkin comparisons above used the Windows pack binary. Linux verification used native harness and bot binaries with Java reference and subject servers.
