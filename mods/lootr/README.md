# Lootr native port

Reference: Lootr 1.25.42.124 for Minecraft 26.3, local source checkout
at `D:/mc-ref/lootr-1.25.42.124`, commit `2a9c830`.
The source license is MIT; original visual/audio assets are not included.
The separate 26.2 build is not the engine target.

## Implemented slice

- All seven source chest/barrel block identities, with 60 exact single-chest
  or barrel states. Copper uses the corresponding waxed physical template.
- Explicit `LootTable` and `LootTableSeed` block-entity fields.
- Independent saved inventories per persistent container identity and player UUID,
  with a stock 27-slot menu and staged take/insert transactions.
- An empty chest starts empty, not with arbitrary dungeon loot.
- Unknown tables fail closed without an inventory claim.
- Site and identity checks reject replaced or copied containers. Live validity
  checks use the host's stock-menu extension; they do not deserialize the
  entire inventory archive on each tick.
- Chest placement opposite the player's horizontal view, source waterlogging,
  and six-direction barrel placement. Spectator and sneak-use cannot create
  private claims; chest obstruction uses the host's chest-blocked query,
  whereas a barrel can open beneath a solid block.
- Source reference inventory NBT can clone slots independently for each
  owner. Plain custom names are preserved in the menu title. Unsupported
  lock predicates are denied rather than bypassed.
- Source default comparator strength 1 for all states. Trapped weak/direct
  redstone power still needs actual open-count support from the host.
- The permission-checked `/lootr id` command reads the identity of the
  container at the player's feet or one block below.
  These additions are pending host-gated verification.

Set `config/lootr/randomise_seed.txt` to `false` to preserve a nonzero,
non-minus-one explicit seed. The default is `true`. With randomisation
enabled, or a seed of zero or minus one, each new inventory uses a fresh
random seed. This matches `ILootrAPI.getLootSeed` and
`LootrCommonConfig.Conversion.randomiseSeed`. The port's configuration file
format is intentionally not a claim of upstream config compatibility.

## Source-derived conversion requirements

The source's `MixinRandomizableContainer` queues eligible block entities when
their table is loaded or assigned, and prevents vanilla unpacking before
conversion. `BlockEntityTicker` filters the block-entity blacklist, dimension
and world-border restrictions, available conversion provider, table blacklist,
loaded chunk neighbourhood and structure restrictions. It rechecks the table
before replacement. The chest provider uses `lootr:convert/chests` (including
`minecraft:chest` and `#c:chests/wooden`).

Safe native conversion requires a host pre-unpack callback and a loaded
candidate stream, plus a compare-and-replace operation that captures the
original loot table/seed, facing, waterlogging and supported container
components, clears the original table before replacing the block, and creates
the new block entity without dropping or rolling original loot. A failure
must leave the original block and its table untouched. The source explicitly
clears the table before replacement to avoid duplicate generated drops.
Scanning every position on every tick is not a substitute.

The mod callback now maps vanilla chest, trapped chest, barrel and both
waxed/unwaxed copper states to their matching Lootr state. The engine
transaction currently covers single ordinary chests; other variants need
the corresponding host-side eligible-container support and live verification.
Team UUIDs, full lock predicates, richer text components, actual opener
signals, visuals, structure/table filtering and break rules remain pending.

### Next source-matched block and lifecycle slices

`LootrConstants.BlockProperties` copies vanilla chest, trapped chest and
barrel with strength 2.5. Copper variants copy the corresponding waxed
vanilla states. `LootrChestBlock.getStateForPlacement` uses the opposite
horizontal direction, forces a single chest and preserves waterlogging.
The collision/outline shape is a 14-by-14 column of height 14 model units.
The chest refuses opening when blocked above, whereas the barrel does not.
The vanilla 26.3 `ChestBlock.isChestBlockedAt` checks redstone conduction
above and a sitting cat intersecting the block above. The port now uses
the generic host chest-obstruction query; its exact behavior is awaiting
host-gated verification.
The exact registry identifiers are `lootr:chest`, `lootr:trapped_chest`,
`lootr:barrel`, `lootr:copper_chest`, `lootr:exposed_copper_chest`,
`lootr:weathered_copper_chest`, and `lootr:oxidized_copper_chest`.
Sneak-use removes the visual opener marker when permitted; it does not
regenerate loot or delete the private inventory. Spectators do not open
private inventories. The archive records the visual opener marker separately
from the private claim; sneak-use clears it without rerolling the inventory.
Client rendering of that marker remains pending.

Upstream refresh and decay are opt-in: both default to 6000 game ticks, with
empty table/dimension filters and both global switches disabled. Start on open
by default, not on an arbitrary ticking container. Enabled expiry processing
also runs while loaded; it must use persisted world game time, not process
uptime. Refresh deletes all private inventories and visual opener markers,
without reusing transaction IDs. Decay destroys the container (default) or
replaces it with its corresponding vanilla block when configured. The source
administrative commands are `/lootr refresh`, `/lootr decay` (stand on
the target) and `/lootr cclear <entities>`; the latter requires the
permission-checked entity argument interface. A strict policy parser and
absolute persisted deadlines now exist, with atomic archive refresh and
non-reused claim IDs. Refresh is wired to the required `GameTime` extension
and refuses access if the persisted overworld clock cannot be read. Loaded
block events rehydrate the timer index, with at most 16 due/probe sites per
dimension per tick. Unloading removes the transient queue entry, not the
saved timer. Open also checks the absolute deadline, so an unloaded
container cannot expose an expired inventory. A successful shard write
invalidates the old menu epoch; late clicks cannot reuse the retired IDs.
The refresh tick does not scan world blocks or decode shards on every
menu validity check.
With the opt-in start-while-ticking filter enabled, claimed but currently
unscheduled sites are reprobed every 200 game ticks (subject to the same
budget) so changed loot-table metadata can start a timer without a block
state change.

Optional `config/lootr/lifecycle.txt` uses strict port-specific key/value
syntax. Missing file means upstream defaults. For example:

```text
# Opt in globally, or use comma-separated filters instead.
refresh_all=true
refresh_value=6000
refresh_perform_while_ticking=true
refresh_start_while_ticking=false
# refresh_loot_tables=minecraft:chests/simple_dungeon
# refresh_loot_table_modids=minecraft
# refresh_dimensions=minecraft:overworld
```

`/lootr refresh` is permission level 2, targets the block at the player’s
feet or one below, and resets its persisted deadline to the configured
delay. Tick expiry requires a prior private claim; opening an already-due
container also refreshes even when tick expiry is disabled. The configured
duration does not extend an existing timer on ordinary re-open. Structure
tag filters and source status notifications are not supported yet.

The new source path supports opt-in no-drop decay with
`replace_when_decayed=true` and requires the host's
`ContentEntityReplacement` extension in that configuration. For example:

```text
decay_all=true
decay_value=6000
replace_when_decayed=true
decay_perform_while_ticking=true
```

`/lootr decay` (permission level 2) can also start a timer when guarded
replacement is enabled. Both open and tick paths use the exact saved
entity snapshot and new vanilla destination NBT; an exposed copper
chest becomes a waxed exposed copper chain as in the source. Default
drop-producing decay is still unsupported: enabled decay filters without
`replace_when_decayed=true` are rejected. A due identity without the
replacement policy denies new claims without touching the block.
This new path is source-only, awaiting gated compilation and runtime
validation; it does not claim cross-file crash-atomicity.

## Persistence scaling

The frozen v1 package uses one save-global key. The v2 Rust source now
implements 64 container-ID-prefix shards, a durable initialization manifest,
lazy legacy migration, and one-shard menu transactions. The shard is encoded
in the non-reused menu ID; site bindings survive inventory resets. Migration
preserves previously claimed empty inventories and allocation high-water
marks. Once all legacy-bearing shards are initialized, the old payload is
retired. The v2 source has passed unit tests and live fresh-world protocol
scenarios. A live v1-to-v2 upgrade still needs verification; see
[PERSISTENCE-DESIGN.md](PERSISTENCE-DESIGN.md).
The host limits a value to 8 MiB, the whole saved-data file to 64 MiB,
and the key count to 4096. Sharding reduces hot-path work but still shares
the global file-size and flush limits. True large-world parity
requires region/container-scoped persistent records and host-atomic per-record
commits, analogous to the source's `LootrSection` organization.

Normal save/restart persistence is the current guarantee. The menu transaction
stages player and container changes together in memory, but the host persists
player inventories and mod saved data through separate normal save paths.
Crash-consistent cross-file commit needs a host journal/checkpoint protocol;
the current archive does not claim that stronger guarantee.

## Scenarios

Verified against the November 27, 2026 development host using the Rust
protocol bot: personal chest, configured generated chest, unknown table,
cursor close, copied identity, replacement with a held cursor, and malformed
identity. These are not graphical client tests. The UUID path uses the
native entity UUID, matching the host's inventory transaction owner.
The generated-chest scenario exercises an explicitly configured Lootr chest,
not automatic vanilla-container conversion.

- `lootr-personal-chest.txt`: empty chest, deposits, separate player inventory,
  restart, withdrawal.
- `lootr-generated-chest.txt`: explicit deterministic test table, concurrent
  players, take isolation, range close, restart, replaced chest identity.
- `lootr-unknown-table.txt`: invalid table refuses to open, repairing the
  table in place permits generation.
- `lootr-cursor-close.txt`: right-click splitting, insertion, closing with
  a held cursor stack, restart and exact item conservation.
- `lootr-copied-identity.txt`: copied UUID at another position denied to
  both existing and new owners; the original chest remains intact.
- `lootr-migration-before.txt` and `lootr-migration-after.txt`: v1 to v2
  same-world upgrade, two shards, preserved empty and partial claims,
  item conservation and another restart. Run the former with the old
  package and `-Keep`, then the latter with the new package and no
  `-Fresh`. Do not downgrade the upgraded save.
- `lootr-chest-states.txt`: player placement facing and source waterlogging,
  obstruction/spectator/sneak denial followed by a first valid loot claim,
  and state persistence across restart.
- `lootr-variants.txt`: explicit variant menus, barrel obstruction exception,
  independent reference inventory clones, and restart persistence.
- `lootr-negative-metadata.txt`: unknown item and duplicate reference slots
  are repaired in place, unsupported lock denies access, valid repaired claim
  persists exhausted on restart while another player receives the full copy.
  The new claim path verifies stack decoding with the host before persistence;
  counts beyond the effective maximum are rejected, not silently truncated.
- `lootr-refresh-restart.txt`: auto refresh across restart, daylight-clock
  changes, two independent exhausted claims and held-cursor menu invalidation.
  Preseed the dedicated run's `config/lootr/lifecycle.txt` with the settings
  in the scenario header and run with `-Keep`, **without** `-Fresh`.
  Requires the new GameTime host. Not yet runtime verified.
- `lootr-refresh-permission.txt`: explicit admin timer with no automatic
  filter, denied ordinary player command, and open-only expiry. Preseed its
  dedicated run configuration as stated in the header.
- `lootr-decay-replace.txt`: guarded no-drop decay of barrel and exposed
  copper chest, exact destination state, empty vanilla inventory, untouched
  unclaimed neighbor, held cursor conservation and restart. Preseed its
  dedicated configuration per the header. Requires the new replacement
  extension and remains unrun.
- `lootr-migration-fixture-setup.txt` and `tests/fixtures/lootr-legacy.py`:
  a schema-derived legacy save fixture for the same migration-after scenario.
  Its provenance and exact commands are in [PERSISTENCE-DESIGN.md](PERSISTENCE-DESIGN.md).

The original `lootr-variants.txt` passed on the cached host on 2026-11-27
at checkpoint `f68c6bf` (retained `run/lootr-variants-f68c6bf/scenario.log`).
On the GameTime host, `cargo test -p lootr` passed all 27 tests and the
release package built. The negative metadata case passed (retained
`run/lootr-negative-gametime-05f0167/scenario.log`). The first refresh
restart case preserved exhausted claims before expiry, refreshed both
players afterward, and conserved the held stick on the server. It exited
1 solely because the bot's cursor assertion requested a cursor packet
after the container had closed (the bot clears its cache on close).
The scenario now asserts an untouched marker in the player inventory
instead, with the exact server item-count check retained. That corrected
scenario and the permission scenario still require gated execution.
Original evidence: `run/lootr-refresh-restart-05f0167/scenario.log`.

`lootr:tests/full_chest` is a port-authored deterministic fixture with 27
unstackable vanilla wooden swords. It is not an upstream loot table.
The full-chest fill removes slot-placement randomness from these assertions;
it does not test Java/Rust RNG sequence equivalence. The scenarios need
the host's container loot and validated-menu interfaces.
