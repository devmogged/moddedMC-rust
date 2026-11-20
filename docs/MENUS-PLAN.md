# Native inventory menus: implementation plan

Status: **design proposal only** (WS-0 M4, 2026-11-20). No interface or gameplay behavior in this document is implemented merely by landing this plan. Freeze a C table only after checking the exact Minecraft Java 26.3 menu/packet lifecycle against decompiled vanilla source and exercising an independent native fixture.

## Why this interface is needed

The existing `Player::show_menu` / `Mod::menu_click` path delivers multi-action **dialogs**, not `AbstractContainerMenu` inventory screens. `moddedmc:containers` v1 exposes a vanilla *block* inventory through NeoForge-style sided item handlers for automation; it does not open a player screen and cannot safely substitute for a viewer-specific inventory. `moddedmc:item_stacks` already supplies named-root saved `ItemStack` NBT and bounded stack operations. Extend those capabilities rather than designing an incompatible item representation.

First downstream clients: Lootr 1.25.42.124's per-player loot containers (WS-13, request [#74](https://github.com/devmogged/mmc-coord/issues/74)), Farmer's Delight cooking and cutting inventories (WS-1, request [#41](https://github.com/devmogged/mmc-coord/issues/41)), Cataclysm fusion (WS-5, request [#61](https://github.com/devmogged/mmc-coord/issues/61)), and Sophisticated Storage/Backpacks (WS-18/19). They have **different inventory ownership policies**; the host must not turn Lootr's independent per-player inventories into a single shared vanilla chest.

## Responsibility split

| Host / Pumpkin | Drop-in mod |
| --- | --- |
| Vanilla Java 26.3 `AbstractContainerMenu` click semantics: pickup/place, split, shift-move, number hotbar swap, drag/quick-craft, pickup-all, throw, outside click, offhand, creative, cursor carried stack | Meaning of each custom slot (input, fuel, output, upgrade, locked), predicates, recipe/process/output logic |
| Menu state IDs, window/session IDs, open/close and packet lifecycle, safe client synchronization and resync on rejection | Backing mod-owned inventory keyed by actual container identifier and optionally resolved player/team UUID |
| Validation of action packets, player inventory/cursor ownership, stack limits, component-aware equality and authoritative transaction application | Mod-provided insertion/extraction permissions, per-slot limits, changed-slot callbacks and durable state |
| Lifecycle cleanup for death, disconnect, teleport, world unload and block removal; reject stale windows | Whether a viewer remains allowed to use a container; what to do on closing and changing an item |
| Source-compatible packet mapping for stock 26.3 clients | Model/screen metadata if a companion is available; do not require companion for a vanilla 9xN inventory |

A mod should never receive or hold a raw Rust `Player`, `World`, `Inventory`, `ItemStack`, `Arc`, or dynamic trait-object pointer across the C boundary. World/player handles are borrowed for one synchronous call only. All serialized stacks use **exactly** the existing saved-item NBT encoding (`id`, `count`, `components`) and `MAX_STACK_BYTES` bound.

## Proposed interface split (names and callback signatures are not yet frozen)

1. **`moddedmc:menus` v1**: register a namespaced menu *kind* during `Mod::register`, before worlds open. The only first-release screen shape is generic `minecraft:generic_9x3` (27 mod slots plus the vanilla player inventory and hotbar). A second register field may provide other vanilla-screen types only after they are individually tested. Registration must reject unknown menu types, another mod's namespace, oversized slot layouts and duplicate ids without partially staging state.
2. **Open request**: mod requests the host to open one registered kind for one live player and one loaded source (world + block/entity id + position, or a strictly scoped logical inventory), with a bounded title and initial slot snapshot. The host returns a *session/window id* or a precise failure. Open must verify the world, distance/permissions and selected container are live before transmitting packets. A pending callback must never silently open a shared inventory for a mod that requested per-player data.
3. **Backing provider callback**: for a menu session, the mod supplies `read_slot`, `may_place`, `may_take`, slot-limit and `commit` actions over opaque bounded saved NBT. The host owns the player's inventory, hotbar and cursor; the mod owns only its custom slots. Either the host fetches an atomic snapshot then commits a checked revision, or it synchronously invokes a provider transaction under one server tick lock. **Do not** call mod callbacks while holding a lock that the callback may reenter. The provider must support a monotonic revision to reject stale saves rather than accepting a best-effort merge that can duplicate an item. It must never return a long-lived pointer into mod memory.
4. **Server-authoritative click dispatch**: host first verifies window id, incoming state id, slot/range/action, stack encoding, carried stack and currently allowed access; then computes vanilla click behavior *once* against one snapshot. It must validate both sides of a shift-transfer, including slot predicates and player storage. A rejected or conflicted transaction makes **zero** player or mod inventory changes and sends an authoritative full resync. A successful transaction atomically commits the mod slots, player slots and cursor then broadcasts only changed slots using vanilla state-id sequencing. A callback can request a re-open or close on semantic changes, but must not recursively run arbitrary clicks.
5. **Close and teardown**: close callback is delivered exactly once after successful open, even for disconnect, chunk unload, teleport, menu replacement and server shutdown. Cursor and carried stacks follow vanilla close semantics, never deleted or duplicated; stale packets after closure are ignored. When a source block or entity changes identity, the session invalidates, not rebinds to a different block at the same coordinates.
6. **Persistence and side access**: the menu is a *view* over an inventory, not its owner. Block-entity or `moddedmc:saved_data` writes remain the mod's responsibility (and must occur before successful commit acknowledgment); vanilla hoppers use the existing `moddedmc:containers` contract. Exposing mod-owned slots to hoppers requires a separate provider extension under [#41](https://github.com/devmogged/mmc-coord/issues/41), not a menu-specific automation shortcut.

### Suggested C-boundary structure (illustrative, **not** public ABI)

A registered kind has a borrowed ID and immutable shape; a session is identified by a host-created integer, never by address. All callback inputs are length-bounded C structs with `size: usize` followed by fields, and all outputs use caller-owned buffers or the existing `Sink` copied synchronously. Version 1 registration should expose a single provider table for snapshot/validate/commit and close; avoid separately mutable `getSlot` + `setSlot` calls during one click unless the host can guarantee one transaction. No dynamic callback pointer may outlive the loaded mod library (libraries currently stay loaded until process exit). Result codes must distinguish invalid input, unloaded/missing source, forbidden player, stale revision, menu already closed and success.

If existing Pumpkin inventory click internals cannot process an external backing provider without invasive duplication, introduce the **smallest generic inventory adapter inside Pumpkin**, then expose it from `engine/moddedmc-host` as this stable table. Reuse the same menu logic vanilla containers use; a second mod-only click interpreter will drift. Do not change the layout of any already published `HostApi`, `ItemsV1` or `ContainersV1`. A new versioned named interface is preferable; append-only extensions require size guards in the SDK.

## First milestone: 9x3 fixture and oracle

Create `tests/fixtures/abi-menus/` with a standalone native provider. It owns two independent 27-slot inventories keyed by a test viewer identity, a per-inventory revision and deterministic initial stacks. Source and destination are known test blocks in loaded chunks. The fixture cannot use `pumpkin_core` or host types. Add `tests/scenarios/abi-menus.txt` with the stock 26.3 bot driving actual container-click packets; extend the bot minimally if a needed click gesture is not implemented.

Accept the first milestone only if all of the following have evidence:

- Opening the fixture yields the exact title, window id, 27 mod slots, 27 main-player slots, 9 hotbar slots and cursor; a different player sees a different mod-owned inventory where configured.
- Left and right pickup/place, splitting odd/even counts, exact max stack and component-sensitive equality agree with the real Java 26.3 `AbstractContainerMenu` using matching input stacks. Shift-click and hotbar swaps never transfer into a slot that rejects the item. Test full-inventory and partial-stack remainders.
- Repeated clicks with the same stale state id, malformed NBT, unknown slots, wrong window, impossible carried stack and creative-only actions in survival **do not mint or destroy items**. For each accepted transaction, compare the multiset of item counts and component identities across mod slots + player inventory + carried cursor before/after, allowing only declared recipe consumption/output changes.
- Two viewers race on a **shared** fixture inventory: exactly one revision wins, the other sees a full resync; a **per-player** fixture inventory remains independent even when the same world chest is viewed.
- Close on disconnect, teleport out of range, block replacement and shutdown; reopening has no ghost slots, double-close or cursor duplication. Stop/restart restores exactly the mod-saved slots and honors the same player identity (first fixture must use a real saved-data path for the persistence test).
- Test a vanilla chest using the unchanged Pumpkin menu implementation before and after the change. Test the existing `moddedmc:containers` v1 insert/extract behavior: menus must not break hoppers or furnaces. Re-run P1a Create and current ABI fixtures.
- `cargo fmt --all --check`, root API tests/Clippy with `-D warnings`, host unit tests, pack build, bot scenario and vanilla 26.3 differential fixture PASS. Measure click hot path allocations and latency before/after once it is implemented; no measurements or parity claims exist yet.

## Staging and downstream follow-up

**Stage A:** identify Pumpkin's 26.3 menu session, state-id, `ClickContainer` packet and inventory adapter points from decompiled vanilla and current `engine/pumpkin`. Write a failing fixture for a real inventory click; no API publication yet.

**Stage B:** introduce host-only adapter and test actual vanilla chest parity; freeze the named C table and add a size-checked SDK wrapper only after malformed-packet, re-entrancy and stale revision contracts are proven. Run the independent native fixture before inviting consumers.

**Stage C:** WS-13 Lootr wires its `InventoryStore` to viewer-scoped provider and to separately requested actual vanilla loot generation (#74). WS-1, WS-5 and WS-18/19 connect their own slot rules and recipes. This menu API does **not** magically implement recipe registration, loot rolling, external automation or generic nonliving entity registration.

### Open questions to resolve against source before freezing v1

- Precisely which 26.3 protocol packet/state-id and quick-craft ordering paths Pumpkin already handles, and which the bot cannot emit.
- Whether a generic 9x3-only first release is enough to validate the provider contract before supporting nonstandard slot counts, crafting output slots and dynamic machine screens.
- How to expose mod-backed inventory state to the existing vanilla `Inventory` lock without nested re-entry or holding locks while invoking an external function pointer.
- Whether vanilla processing offers a transaction dry-run suitable for two-phase commit; if not, what minimal common transaction primitive prevents dupes when a saved-data write fails.

Decisions should be recorded here with source file/method names and real fixture outcomes. Until then, this is a roadmap, **not** a promise of a usable `moddedmc:menus` interface.
