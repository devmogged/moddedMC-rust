# Differential harness

The harness decides whether something is done. It runs one scenario against a **reference** server and a **subject** server and reports every difference. For vanilla behavior the reference is the official 26.3 server. For a port, it is the original mod on its own loader. The subject is a pack server built from this repo.

"It compiles", "the unit tests pass" and "it looks right in game" are not evidence of parity. A harness run with no unexplained differences is.

## Goals

- **Tick-exact.** Both servers run frozen (`/tick freeze`) and advance with `/tick step <n>`, so "after 40 ticks" means the same thing on both.
- **Deterministic.** Scenarios turn off what neither side can reproduce, unless that behavior is the thing under test: `doMobSpawning`, `randomTickSpeed 0`, `doWeatherCycle`, `doDaylightCycle`, fixed seed, fixed spawn. A scenario that tests randomness compares distributions over many runs, not single outcomes.
- **No changes to the reference.** Vanilla runs unmodified. Everything the harness learns about the reference comes from its console, its saved world and a bot's packets.
- **Readable failures.** A mismatch names the position, the field, the reference value and the subject value, plus the scenario line that produced it.
- **Cross-platform and agent-friendly.** The Rust binary is in `engine/harness/` (crate `moddedmc-harness`). Output is human-readable by default and JSON with `--json`.

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

The reference jar never goes into the repo. Each machine points at its own copy. The harness prewarms the vanilla bundler under the jar's adjacent `META-INF` directory, shared by both servers and all workers. Place the jar in a writable cache outside the run directories. Each scenario gets its own ports and world.

### F2 implementation

Build the harness and its bot from the repository root. Use the provided or separately built `pack-dev` for the subject.

```powershell
$env:PUMPKIN_MINECRAFT_SERVER_JAR = 'D:/mc-ref/26.3/server-bundler.jar'
cargo build --manifest-path engine/Cargo.toml --profile pack -p moddedmc-harness -p moddedmc-bot
engine/target/pack/moddedmc-harness.exe run-all tests/harness --reference vanilla:D:/mc-ref/26.3/server-bundler.jar --subject engine/target/pack/pack-dev.exe --jobs 2
engine/target/pack/moddedmc-harness.exe self-test --reference vanilla:D:/mc-ref/26.3/server-bundler.jar
```

`--bot` overrides the bot executable next to the harness. `--run-root` overrides `run/harness`. Each checkpoint preserves both consoles, bot logs and worlds. `run-all` finds `.txt` scenarios recursively and prints in path order. The default worker count is half the available logical CPUs, with a minimum of one. `--jobs` accepts up to the available CPU count.

Exit 0 means every check passed. Exit 1 means at least one unexplained difference. Exit 2 means a parse, launch, synchronization or snapshot error prevented a verdict and no mismatch was recorded. JSON carries the scenario and error separately from the mismatch and allow records. The `self-test` command passes only when the identical run matches and the deliberate mutation produces the exact expected mismatch.

**Timing and snapshots.** Ready, freeze and the first readable game clock establish each side's scenario tick 0. Both vanilla and Pumpkin report step acceptance. Their `tick query` continues to say frozen during a step, so the harness also polls `time query gametime` to the exact requested relative value. An overrun, a stalled clock or a failed freeze fails the run. The harness checks both clocks again before each subsequent step or capture. Startup uses seed 12345 unless overridden, a fixed world spawn at `0 150 0`, respawn radius 0, and the four deterministic game rules. The comparison area is force-loaded and checked before any scenario command.

Pumpkin's `Saved the game` can precede its async Anvil writes. Each snapshot sends `save-all flush`, waits for confirmation and captures live player saves, then gracefully shuts down both sides before opening their region files. Every later checkpoint replays the same prefix from fresh worlds. Earlier snapshot steps still issue the save command. Console assertions in a prefix are reported at their first checkpoint. The on-disk reader fails on missing or partial terrain and malformed NBT. An absent entity region represents an empty entity set.

**Comparison rules.** `area` is an inclusive overworld block box within the 26.3 build height, capped at 256 chunks and 1,048,576 blocks. Entity anchors lie in the corresponding half-open spatial box. Block palettes use the vanilla 26.3 default-state strings and `id/properties` maps or Pumpkin's `Name/Properties` maps. Comparison fills known default properties from the generated 26.3 registry and retains explicit properties and unknown identifiers. Entities with the same type and position within `1e-6` per axis are matched as a multiset, preferring equal payloads before reporting mismatches. NBT compound key order is immaterial, list order and tag types are retained, and only doubles use the tolerance.

The default ignored NBT keys are `UUID`, `UUIDMost`, `UUIDLeast`, `LastUpdate`, `InhabitedTime` and `LastPlayed`. Gameplay timers and missing-versus-present tags are compared. Player checks cover position, rotation, dimension, health, food, XP progress and totals, effects, inventory, ender items, equipment and held slot. `allow` requires an inline explanation and matches full field paths such as `block_entity.keepPacked`. The report lists every allowed difference and its reason.

**Scenario input.** `properties` accepts shared settings for difficulty, gamemode, hardcore, view distance, simulation distance, spawn protection and player limit. The harness owns ports, seed, world directory and test networking. `probe` supports read-only console queries and compares their full feedback, including error text, after removing log prefixes. `expect` consumes a matching line from each server's unread scenario console output. Console fences read a reserved empty storage path and keep their own output out of comparisons. The harness waits for a server acknowledgement before returning from bot join, so a console teleport can follow safely. Bots support `await-teleport`, `use x y z`, `place x y z` and `break x y z`; use and place target the top face. Placement and breaking require a server sequence acknowledgement. The baseline additionally probes the changed block so an acknowledgement alone cannot pass the test.

**Packet windows.** A snapshot with the `packets` suffix requires a joined bot. Every snapshot in that scenario closes a packet window with a reserved `tellraw` marker that the bot acknowledges after receiving earlier packets. An ordinary snapshot discards that window after updating the entity map. The first window starts when the bot enters the play state. The harness compares the order, count, kind and normalized payload of the remaining packets. It fails if capture exceeds 32 MiB in one window.

The entity map binds the login entity to the observing player. Spawned entities are matched by entity type, exact spawn coordinates and occurrence at those coordinates. The map follows observed removals and survives snapshot boundaries. The decoder remaps packet-level entity references, including passengers, damage sources and projectile owners, and strips the spawn UUID. References to unseen entities and IDs embedded in opaque metadata retain their original wire values. Sound seeds, teleport acknowledgement tokens and the absolute game clock are omitted. Empty clock heartbeats are dropped. Clock updates and relative tick checks remain.

Capture excludes keepalive and ping traffic, bundle and chunk batch delimiters, initial chunk and biome blobs, and recipe and tag registry broadcasts. Block updates, standalone light updates, particles, sounds, entity packets, inventory and other play packets are retained. Large difference values have a byte preview and SHA-256 digest. Inserted or missing packets can shift subsequent indices.

Move/look/attack and inventory-click synchronization and cross-version mod-oracle launch and ID maps are open work. A scenario that requests an unsupported bot action fails explicitly.

## Done means

A system or port is done when its scenario set passes with no unexplained `allow`, and it also passes the performance gates in [ROADMAP.md](ROADMAP.md).
