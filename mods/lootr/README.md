# Lootr native port

Reference: Lootr 1.25.42.124 for Minecraft 26.3, local source checkout
at `D:/mc-ref/lootr-1.25.42.124`, commit `2a9c830`.
The source license is MIT; original visual/audio assets are not included.
The separate 26.2 build is not the engine target.

## Implemented slice

- `lootr:chest` (the 26.3 identifier in `LootrConstants.Identifiers`).
- Explicit `LootTable` and `LootTableSeed` block-entity fields.
- Independent saved inventories per persistent container identity and player UUID,
  with a stock 27-slot menu and staged take/insert transactions.
- An empty chest starts empty, not with arbitrary dungeon loot.
- Unknown tables fail closed without an inventory claim.
- Site and identity checks reject replaced or copied containers. Live validity
  checks use the host's stock-menu extension; they do not deserialize the
  entire inventory archive on each tick.

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

This conversion is not implemented yet. Other pending gameplay includes
barrels, trapped/copper chests, reference inventories, locked containers,
team UUIDs, redstone/comparators, visuals, source refresh/decay scheduling,
structure and table filtering, and break rules. Refresh/clear archive
primitives exist, but no gameplay command or schedule invokes them yet.

### Next source-matched block and lifecycle slices

`LootrConstants.BlockProperties` copies vanilla chest, trapped chest and
barrel with strength 2.5. Copper variants copy the corresponding waxed
vanilla states. `LootrChestBlock.getStateForPlacement` uses the opposite
horizontal direction, forces a single chest and preserves waterlogging.
The collision/outline shape is a 14-by-14 column of height 14 model units.
The chest refuses opening when blocked above, whereas the barrel does not.
The exact registry identifiers are `lootr:chest`, `lootr:trapped_chest`,
`lootr:barrel`, `lootr:copper_chest`, `lootr:exposed_copper_chest`,
`lootr:weathered_copper_chest`, and `lootr:oxidized_copper_chest`.
Sneak-use removes the visual opener marker when permitted; it does not
regenerate loot or delete the private inventory. Spectators do not open
private inventories. The current chest is a one-state carrier; these placement
and interaction differences are still to be ported.

Upstream refresh and decay are opt-in: both default to 6000 game ticks, with
empty table/dimension filters and both global switches disabled. Start on open
by default, not on an arbitrary ticking container. Enabled expiry processing
also runs while loaded; it must use persisted world game time, not process
uptime. Refresh deletes all private inventories and visual opener markers,
without reusing transaction IDs. Decay destroys the container (default) or
replaces it with its corresponding vanilla block when configured. The source
administrative command is `/lootr cclear <entities>`, not an unrestricted
player command. Exposing it requires permission-checked entity arguments.

## Persistence scaling

The frozen v1 package uses one save-global key. The v2 Rust source now
implements 64 container-ID-prefix shards, a durable initialization manifest,
lazy legacy migration, and one-shard menu transactions. The shard is encoded
in the non-reused menu ID; site bindings survive inventory resets. Migration
preserves previously claimed empty inventories and allocation high-water
marks. Once all legacy-bearing shards are initialized, the old payload is
retired. The v2 source still requires controlled compilation and live
upgrade verification; see [PERSISTENCE-DESIGN.md](PERSISTENCE-DESIGN.md).
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

`lootr:tests/full_chest` is a port-authored deterministic fixture with 27
unstackable vanilla wooden swords. It is not an upstream loot table.
The full-chest fill removes slot-placement randomness from these assertions;
it does not test Java/Rust RNG sequence equivalence. The scenarios need
the host's container loot and validated-menu interfaces.
