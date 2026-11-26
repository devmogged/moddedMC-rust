# Scalable archive implementation

Phase 1 is implemented in Rust, pending controlled compilation and runtime
verification. The current on-disk package snapshot remains the validated
v1 baseline. The implementation does not require a new host interface.

## Why change it

In the original v1 implementation, every open and menu mutation decodes, validates, and serializes
the complete Lootr archive under a global mod mutex. Even a single slot
change scales with the total number of players and containers. The host
limits each saved value to 8 MiB (`api/src/abi.rs:1761`), the complete
saved-data file to 64 MiB and 4096 keys
(`engine/moddedmc-host/src/saved_data.rs:24-25,268-290`).
The existing host flush snapshots the entire save under its mutex before
writing and checks the revision after flush (`saved_data.rs:348-367`).

## Phase 1: fixed shards using existing transaction ABI

Register 64 stable keys, `inventories/v2/00` through
`inventories/v2/3f`, alongside the existing `inventories` key.
The additional `inventories/v2/manifest` records initialized and
legacy-bearing shard bitmaps, the old allocation high-water mark, and
whether legacy compaction completed.
Derive a shard directly from the leading six bits of the canonical
32-hex-digit persistent container ID; do not use Rust's process-random
hashing or current coordinates. Every player record for one container
belongs to the same shard. The current record format and integrity
checks are retained initially.

Encode a runtime menu identifier as a shard plus a positive local
record ID: bits 56..61 select the shard, bits 0..55 are the local
ID. Reject zero or out-of-range local IDs and reserved high bits.
The callback obtains the shard from `Change.inventory` without scanning
archives, and validates the owner and source site as before.
Only that shard's key is committed through `Change::commit`.
No transaction crosses two shards; global player-clear is an administrative
operation with explicit multi-shard completion/failure reporting.

Keep the current global mutex until host concurrency behavior and
migration are verified. Per-shard mutexes can then reduce contention;
the host's own saved-data lock still serializes actual writes.
Maintain the existing immutable menu validity context so tick validation
does not read any archive.

## Migration without resetting previously claimed loot

Do not overwrite the legacy archive during the first open. Under the
global lock, if a shard value is absent, decode and validate the legacy
archive; copy every record for that shard into a versioned initialized
shard. Preserve record IDs and the original allocation high-water mark.
An existing shard is authoritative, including a valid empty shard after
refresh; never fall back to legacy merely because a record is absent.
Write the initialized shard and its manifest bit successfully before
opening a screen or rolling any new loot. If either write fails, reject
the open. An initialized shard missing from storage fails closed even
before compaction; a shard written before an interrupted manifest write
is reused, not remigrated. Manifest and shard versions are validated.

Read and validate legacy data even when a player first opens a different
new chest in the same shard. A damaged legacy archive cannot be treated
as absent. The host's current read interface conflates missing values
with failure, but write fails when its outer saved-data file cannot load.
An inner legacy decode failure must explicitly reject initialization.

Initialization is idempotent and may complete incrementally. Keep the
legacy key immutable until every shard that actually contains legacy
data has initialized and been validated. Only then replace it with
an explicit migrated marker (not a missing key). Untouched empty shards
can initialize afterward using the persisted legacy high-water mark.
The host's atomic whole-file flush observes either an earlier consistent
snapshot or a later consistent snapshot; no marker can become visible
before its shards if all operations use the same host and global mod
lock. Avoid compaction if any shard initialization fails or the host
capacity would be exceeded. Report migration capacity failures rather
than discarding old records. Legacy plus new copies can temporarily
increase disk/memory usage. The manifest makes failed compaction retryable
on a later open, without rereading legacy on every normal inventory click.
The upgrade is one-way: do not run the v1 package against a save after v2
has accepted transactions. During partial migration its legacy snapshot is
stale; after compaction v1 correctly rejects the marker as invalid NBT.

Container-site bindings are retained separately from owner records. Legacy
record sites are promoted during partitioning, and a refresh or last-owner
clear never releases a bound UUID for use at another position.

This protocol covers orderly save/restart, not cross-file crash-atomic
player-item persistence. That is already a separate host limitation.

## Validation

* Unit-test shard routing for all 64 prefixes, boundary local IDs,
  wrong-owner and wrong-shard rejection, no ID reuse after refresh.
* Migrate a legacy fixture containing two owners, empty claimed slots,
  and multiple shards; verify no roll after empty claim migration.
* Simulate partial migration followed by restart, repeated initialization,
  malformed legacy records, malformed shard and failed write.
* Once initialized, a missing or malformed shard must fail closed,
  even before compaction. Untouched shards may initialize empty.
* Run all five stock-protocol Lootr scenarios unchanged. The two-phase
  `lootr-migration-before.txt` and `lootr-migration-after.txt` scenarios
  exercise an empty claimed inventory with a subsequently assigned table,
  partially consumed loot for a different player, and deposited items in
  a second shard. Run before with the frozen v1 package and `-Keep`, then
  run after with the v2 package, same run directory, no `-Fresh`.
  Verify multiple shards and exact item conservation across another restart.
  The before scenario now explicitly loads the frozen local package at
  `run/lootr-v1-package/lootr.mmc` rather than whatever current package is
  under `target/mods`. As inspected on 2026-11-27, it is 159,628 bytes,
  SHA-256 `f5ee920b6b81075d29e2ee547a75f93e8a63589aee94de29af545bc1ea4bf512`.
  Its original source/build commit is not recorded in git; the earlier
  v1 diagnostic worlds have no saved-data file. Do not treat that package
  as a proven working legacy producer.
  On 2026-11-27 the frozen package was run unmodified on the cached
  03:04:43 host in `run/lootr-v1-authentic-producer`. The producer
  scenario exited 1 (five failed checks). The explicit container-only
  slot 54 assertion failed; subsequent bot logs show menu 0, no open
  container, and no swords received by Bob. The stopped world contains
  only a 93-byte `world/moddedmc/registry.dat`, not `saved_data.nbt`.
  The copied DLL package and pristine package have the same SHA-256.
  This rules out claiming an authentic v1-to-v2 upgrade from that run.
  The symptoms agree with the historical UUID path failure, but that
  frozen binary is uninstrumented and the precise cause is not proven.
  Preserve the producer run and its `scenario.log`; do not seed it
  synthetically or attempt an upgrade as if it were a valid v1 save.
* A second, explicitly synthetic migration fixture follows the actual v1
  `next_id` / `records` schema described by the existing migration unit
  test, the saved-data host envelope, and native stack NBT. It encodes
  Alice's empty claim and seven deposited diamonds, and Bob's 26 remaining
  swords after one was taken. The native UUIDs were observed in the
  passing `run/lootr-v2-personal-uuid/world/players` save. Run these steps
  sequentially with a stopped server between them:

  ```powershell
  scripts\scenario.ps1 -Scenario tests\scenarios\lootr-migration-fixture-setup.txt -RunDir run\lootr-migration-fixture -Fresh -Keep
  py -3 tests\fixtures\lootr-legacy.py
  scripts\scenario.ps1 -Scenario tests\scenarios\lootr-migration-after.txt -RunDir run\lootr-migration-fixture -Keep
  ```

  Use the prime-assigned port and executable options on both scenario runs.
  The generator checks saved player UUIDs and refuses to overwrite an existing
  archive. This exercises the v2 migration reader with independent fixture
  bytes; only a successful historical-package before/after run would verify
  a real old-package upgrade.
* The synthetic 512-record test compares the encoded size of an eight-record
  shard with the global archive and requires a greater-than-32-fold reduction.
  This checks payload scaling, not runtime latency; actual runtime
  benchmarking remains pending. No runtime file reads per tick.

## Phase 2: production-scale persistence

### Refresh lifecycle

Refresh deadlines share the corresponding archive shard with private claims.
The live index is reconstructed from loaded-block events; no queue file or
separate write can diverge from the saved claim state. A successful refresh
removes claim records, retains the binding and allocation high-water mark,
and clears the refresh timer (or begins the next timer when a player opens
the expired container). Only after the shard write succeeds is the loaded
site's menu epoch invalidated. The stock menu closes using its existing
validity mechanism. A failed write leaves the old timer/claims authoritative
and is retried with a bounded delay. Stale clicks cannot resurrect deleted
IDs, even if menu closure has not yet reached the client.

The index bounds expensive due/rehydration probes to 16 per dimension per
tick, deduplicates positions and drops unloaded entries. Each probe reads a
published shard, never initializes an empty shard or adopts an orphan.
The persisted clock is mandatory; daylight time and process ticks are not
substitutes. This is not a cross-file crash-atomic player inventory journal,
and destructive decay remains disabled pending guarded block replacement.

### Guarded decay contract (not yet active)

The upstream `ILootrBlockEntity.performDecay` destroys with drops when
`replace_when_decayed=false`. When true it destroys without drops and sets
the replacement's default state with common properties copied. The Rust
variant planner now derives the exact target selector for all 60 registered
states. The exposed copper type specifically replaces with
`minecraft:waxed_exposed_copper_chain[axis=y,waterlogged=...]`: the default
vertical axis is retained, waterlogging is copied, and chest facing/type
are not shared. All other supported replacements use their normal physical
counterpart with the source properties.

Required host operation: one synchronous loaded-world compare-and-destroy
or compare-and-replace call taking position, expected state, process-local
incarnation and complete expected entity NBT, plus optional replacement
state and drop flag. Under the world mutation exclusion boundary, verify
the loaded entity and its exact metadata, validate the target before any
destructive work, then perform the operation with ordinary update/drop
semantics. A failed comparison must leave the world, drops and block entity
unchanged. No separate unguarded destroy and set-block operations, even
after an optimistic snapshot check. A replacement receives fresh vanilla
entity data, not the old Lootr UUID, table, seed or private inventory.

Under the Lootr archive lock, evaluate decay before refresh, require the
persisted clock and the site's matching bound identity, retain the entire
snapshot, and resolve the replacement before calling the CAS. Do not
consume the deadline, delete claims or invalidate a menu on CAS failure.
Reprobe a still-loaded identity with bounded backoff and a fresh snapshot.
After success, invalidate the old site's menu epoch. Preserve the old
UUID binding and due deadline as a fail-closed tombstone; do not recycle
its menu IDs, or clear the old deadline before the world transition is
durable. The source decay operation does not explicitly delete its saved
inventory store, so retaining claims is safer than discarding them before
a durable world save. A copied old UUID cannot become a new claim at a
different site. A newly placed container gets a new UUID.

The above does not promise crash-atomic world/drop/player persistence:
if the world and item drops can be independently saved, replay of a
pre-decay world after a process crash can repeat drops. Exactly-once
cross-file durability requires the host's shared journal or checkpoint
transaction. Until the guarded operation is provided and validated,
keep new decay policies disabled and deny opening an already-due identity.

Regression plan: reject an outdated snapshot after same-state replacement,
NBT edit or chunk unload without any drop; test default drop versus
no-drop replacement (including all variant properties), independent
player views, stale cursor closure, and orderly restart with unchanged
unrelated blocks. Do not classify those checks as crash-atomic coverage.

Static sharding improves hot-path work, but it cannot remove the
64 MiB aggregate limit or whole-file flush. The production host should
offer a generic registered record namespace with bounded dynamic
record keys, a per-record transaction commit, and enumeration/prefix
iteration for administrative operations. Store per-container or
region-bucketed data, comparable to the upstream `LootrSection` and
`LootrInventoryStore` organization. A record version/revision and
atomic journal/checkpoint should coordinate player inventory changes
and persistent container mutations if crash-level conservation is
required. Host changes should be shared infrastructure, not Lootr-only
file writes bypassing transaction authority.
