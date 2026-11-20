# WS-12 -> WS-0: atomic player-death inventory claim

## Original call and reason

Latest Corpse 1.1.22+26.3 source: `D:\mc-ref\corpse-1.1.22-26.3\src\main\java\de\maxhenkel\corpse\events\DeathEvents.java`, method `playerDeath(PlayerDeathEvent)`, triggered by Corelib. It stores the Death record when enabled, removes ordinary death drops, creates a corpse holding every carried item, and adds that entity to the level. Exact pivotal Java calls: `event.removeDrops()` and `level().addFreshEntity(CorpseEntity.createFromDeath(...))`. Without a pre-drop, server-authoritative handoff the Rust port either duplicates items or loses them.

Checked `docs/MOD-ABI.md` and `api/src/abi.rs` (current origin/main): no registered player-death callback, no atomic player-inventory snapshot/claim. `moddedmc:item_stacks` is for *world item entities*; `moddedmc:containers` is for block inventories; neither closes this gap.

## Proposed append-only C interface

Add `moddedmc:player_death` v1, obtained via `HostApi::get_interface` (no changes to existing field offsets). One possible exact C-compatible table:

```rust
#[repr(C)]
pub struct PlayerDeathInterfaceV1 {
    pub size: u32,
    pub on_death: unsafe extern "C" fn(
        registry: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        callback: unsafe extern "C" fn(
            context: *mut std::ffi::c_void,
            event: *mut std::ffi::c_void,
            player: moddedmc_api::abi::PlayerRef,
            world: *const std::ffi::c_void,
        ),
    ) -> i32,
    pub inventory_snapshot: unsafe extern "C" fn(
        event: *mut std::ffi::c_void,
        entries: moddedmc_api::abi::Sink,
    ) -> bool,
    pub claim_drops: unsafe extern "C" fn(event: *mut std::ffi::c_void) -> bool,
}
```

`inventory_snapshot` invokes `Sink::put(ctx, key, value)` once for every occupied death slot. `key`: 0–35 main, 36–39 armor, 40 offhand (and a documented separate namespace for modded extra inventories); `value`: complete Java-named-root binary `ItemStack` NBT, max 64 KiB per stack. All bytes are borrowed until `put` returns. A result of `false` means the event reference is invalid or any stack was unreadable; return nothing partial. Empty slots are omitted. The snapshot includes owner UUID/name, death dimension and exact position (in event data or separate borrowed reads), as well as inventory content. Host owns event and world handles; both expire after callback.

`claim_drops` succeeds at most once per event. On success, suppress exactly the vanilla item drops represented in the snapshot; on failure leave vanilla drops intact and do not clear player inventory. It must run on the authoritative server thread during the callback, after death drop rules/keep-inventory have been resolved but before drops are spawned. Return `false` for stale/invalid events, already claimed drops, or when keepInventory leaves no droppable contents. A newly spawned corpse or persisted mod state must exist *before* caller claims, to avoid loss.

If host uses a different ABI shape, the required contract is the same: one borrowed death snapshot and atomic suppression, independent of Pumpkin Rust type layout. Do not expose raw `Vec<ItemStack>` across the ABI.

## Acceptance scenarios

1. Death with hotbar/main/armor/offhand contents -> exactly one corpse receives the original slots, zero matching world drops, and the player's post-respawn inventory is empty. Repeat with game-rule keepInventory (do not steal kept items).
2. Death when callback declines or fails -> normal vanilla drops are preserved, no corpse inventory is claimed.
3. Restart with corpse saved -> contents intact once, no duplicate on reload.
4. Two players dying the same tick -> identities, positions, inventories and callbacks do not cross.
5. Failure on corrupt/missing NBT/unloaded world -> no partial claim; invalid borrowed event accesses fail closed.

Further WS-12 needs separate **nonliving custom entity** and **slot-menu/player inventory** APIs; this request is only the death/drop hook.
