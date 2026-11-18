# Differential harness

The harness decides whether something is done. It runs one scenario against a **reference** server and a **subject** server and reports every difference. For vanilla behavior the reference is the official 26.3 server. For a port, it is the original mod on its own loader. The subject is a pack server built from this repo.

"It compiles", "the unit tests pass" and "it looks right in game" are not evidence of parity. A harness run with no unexplained differences is.

## Goals

- **Tick-exact.** Both servers run frozen (`/tick freeze`) and advance with `/tick step <n>`, so "after 40 ticks" means the same thing on both.
- **Deterministic.** Scenarios turn off what neither side can reproduce, unless that behavior is the thing under test: `doMobSpawning`, `randomTickSpeed 0`, `doWeatherCycle`, `doDaylightCycle`, fixed seed, fixed spawn. A scenario that tests randomness compares distributions over many runs, not single outcomes.
- **No changes to the reference.** Vanilla runs unmodified. Everything the harness learns about the reference comes from its console, its saved world and a bot's packets.
- **Readable failures.** A mismatch names the position, the field, the reference value and the subject value, plus the scenario line that produced it.
- **Cross-platform and agent-friendly.** It's a Rust binary (`harness/`, crate `moddedmc-harness`), not a PowerShell script, and it runs on Windows and Linux. Output is human-readable by default and JSON with `--json`.

## Scenario files

These extend the existing `tests/scenarios` format. One step per line, `#` comments.

```
# tests/harness/redstone/repeater-chain.txt
seed 12345
area -32 60 -32 32 80 32           # region to compare (min xyz, max xyz)
gamerules deterministic            # the default set above
start
cmd fill -10 64 0 10 64 0 minecraft:stone
cmd setblock -10 65 0 minecraft:lever[face=floor]
cmd setblock -9 65 0 minecraft:repeater[facing=west,delay=2]
tick 2
cmd setblock -10 65 0 minecraft:lever[face=floor,powered=true]
tick 1
snapshot powered-1                 # compare now, under a name
tick 40
snapshot settled
bot join alice                     # bots use moddedmc-bot
bot alice use 0 65 0
tick 5
snapshot after-use
stop
```

Steps:

| Step | Meaning |
| --- | --- |
| `seed`, `area`, `gamerules`, `properties k=v` | Setup, before `start`. |
| `start` / `stop` | Start both servers frozen at tick 0 and wait until ready; stop both. |
| `cmd <line>` | The same console line on both. |
| `tick <n>` | `/tick step <n>` on both, then wait until both have finished. |
| `bot join <name>`, `bot <name> <action>` | Same bot actions on both (move, look, use, attack, place, inventory click). |
| `snapshot <name>` | Capture state on both and diff it (see below). |
| `expect <regex>` | A console line must match on both. |
| `probe <command>` | Run a query command (`data get block ...`, `execute if ...`) on both and compare the output text. This is how a port compares mod-specific values. |
| `allow <field-glob>` | Ignore a difference for this scenario, with a reason in a comment. Every `allow` is reviewed. |

## What a snapshot compares

1. **World.** `save-all flush` on both, then read the region files for `area` and compare per block position: block state, block entity NBT, and entity NBT (matched by type and position, not UUID). The subject is configured to save in Anvil during harness runs, so both sides go through the same reader (`pumpkin-nbt` and the Anvil reader). Always ignored: UUIDs, `LastUpdate`, `InhabitedTime` and other timestamps.
2. **Players.** Position, rotation, health, food, XP, effects and inventory for every bot, from the saved player data.
3. **Packets.** Optional (`snapshot <name> packets`): the normalized packet stream each bot received since the previous snapshot, compared by kind and payload, with entity ids remapped. Use this for things only the client sees (particles, sounds, block events).

## Mod oracles

The original mod runs on its own MC version (Create 6.0.10 on NeoForge 1.21.1, Saints Dragons on 1.20.1), so world diffs against vanilla 26.3 aren't meaningful there. Port scenarios compare **mod behavior** instead:

- **Probes** of the mod's own block entity and entity data (`data get block <pos> Speed`, `data get entity ...`).
- **Ports keep the original mod's NBT field names and units**, so the same probe works on both sides. This also keeps the door open to importing worlds from the Java mod.
- Scenario lines that differ between the two versions (a block that was renamed between 1.21.1 and 26.3) go through a per-scenario id map.

## Running it

```
moddedmc-harness run tests/harness/redstone/repeater-chain.txt \
    --reference vanilla:D:/mc-ref/26.3/server-bundler.jar \
    --subject engine/target/pack/pack-dev.exe
moddedmc-harness run-all tests/harness --reference ... --subject ...   # summary table
```

The reference jar never goes into the repo. Each machine points at its own copy. For speed, the harness runs scenarios in parallel, each on its own ports and directories.

## Done means

A system or port is done when its scenario set passes with no unexplained `allow`, and it also passes the performance gates in [ROADMAP.md](ROADMAP.md).
