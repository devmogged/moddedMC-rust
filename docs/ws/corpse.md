# WS-12 — Corpse (M0 survey and M2 policy groundwork)

## Release and provenance (2026-11-20)

- Upstream: [henkelmax/corpse](https://github.com/henkelmax/corpse); author Max Henkel.
- Latest published 26.3 build found: **Corpse 1.1.22+26.3**, NeoForge, released 2026-10-22, CurseForge file **9289341** ([release](https://www.curseforge.com/minecraft/mc-mods/corpse/files/9289341)).
- Verified the checked-out upstream source at `D:\mc-ref\corpse-1.1.22-26.3`: commit `12881669fb0ef95e0a863a524bc92674cc2f5608` (2026-10-22), `gradle.properties` declares `minecraft_version=26.3`, `mod_version=1.1.22+26.3`.
- Verified the *published* 252,237-byte jar, stored **outside version control** in `cache/corpse-neoforge-1.1.22+26.3.jar`: valid ZIP with 184 entries, including `CorpseMod.class`, `CorpseEntity.class`, `ServerConfig.class`, NeoForge metadata, and assets.
- **License restriction:** upstream `META-INF/neoforge.mods.toml` and the published jar explicitly declare **All rights reserved**; no separate license file was found at the source root or among inspected jar entries. All original code and assets remain outside our public repository. The owner previously reported mod-port permissions have been obtained; publication of an adaptation and redistribution of assets must remain within the actual authorization. Do not import its art or Java into our public Git history.
- Upstream core dependency: `de.maxhenkel.corelib` (Death, DeathManager, PlayerDeathEvent, ItemListInventory). Its death-storage formats and event semantics need a separate upstream-source audit before M2.

## Complete registered-content inventory from the 26.3 sources

| Kind | Upstream registrations or behavior | Host status |
| --- | --- | --- |
| Blocks | None | N/A |
| Block entities | None | N/A |
| Items | None | N/A |
| Recipes and loot tables | None | N/A |
| World generation | None | N/A |
| Entities | `corpse:corpse` (nonliving, 2×0.5, tracking distance 128, updates every tick); owner UUID/name, skeleton flag, model, equipment sync | **Missing**: arbitrary nonliving mod entity registration, save/load and entity-use callback; `moddedmc:mobs` v1 only supports living mob carriers; `moddedmc:entities` v1 can manipulate existing entities |
| Menus | `corpse:corpse_inventory`, `corpse:corpse_additional_items`; transfer to matching player slots and navigate overflow pages | **Missing**: real slot-based player inventory menus/transactional transfer; basic `Player::show_menu` buttons do not supply inventory slots |
| Commands | Operator-level `/deathhistory <player>` and `/deathhistory <player_uuid>`; normal history opened from client key | **Partial**: `moddedmc:commands` v1 handles roots and integer/literal nodes, not arbitrary player and UUID arguments or remote history GUI |
| Serialization | Equipment-map and UUID entity data serializers; entity NBT `Death`, `Age`, `EmptyAge` | **Partial**: `moddedmc:entities` v1 entity data requires an existing entity; `moddedmc:saved_data` v1 can save global history |
| Player death | Corelib `PlayerDeathEvent` stores death history, takes all vanilla drops, then spawns CorpseEntity | **Missing**: cancellable death/drop event and atomic inventory ownership transfer to a corpse |
| External integration | Optional Jade/WAILA corpse metadata | **Missing / deferred**: separate client integration |

No registered custom blocks, items, block entities, crafting recipes, loot tables or worldgen features exist in the checked upstream 26.3 project. **M1 is therefore vacuous**, not an excuse to add placeholder content.

### Custom payloads (seven)

| ID (`corpse:` prefix) | Flow | Purpose |
| --- | --- | --- |
| `open_additional_items` | serverbound | Open an overflow-items inventory page |
| `open_history` | clientbound | Send the serialized death-history list |
| `request_death_history` | serverbound | Ask for the caller's death history |
| `show_corpse_inventory` | serverbound | Show a historic corpse's items |
| `spawn_death_particles` | clientbound | Removal smoke particles |
| `switch_inventory_page` | serverbound | Page the additional-items menu |
| `transfer_items` | serverbound | Move items back to player inventory |

These payloads belong to the original NeoForge client. Our **stock Java client** cannot be assumed to recognize any of them; an interoperable companion/vanilla-compatible UI contract is required before claiming client parity.

### Server configuration (10 keys)

| Key | Default | Semantics |
| --- | --- | --- |
| `corpse.access.only_owner` | false | Limit opening to owner or admin |
| `corpse.access.skeleton` | false | Allow everyone once skeleton, only when owner-only restriction enabled |
| `corpse.despawn.time` | 600 ticks | Remove an *empty* corpse after the first empty tick plus this delay |
| `corpse.despawn.force_time` | -1 | Force removal after this many ticks even with items; nonpositive disables |
| `corpse.skeleton_time` | 72,000 ticks | Change model to skeleton at this age |
| `corpse.lie_on_face` | false | Visual corpse orientation |
| `corpse.show_equipment` | true | Visible worn equipment |
| `corpse.fall_into_void` | false | Prevent falling below dimension minimum |
| `corpse.lava_damage` | false | Allow removal on sufficient fire damage |
| `death_storage_duration` | -1 days | Keep historical deaths forever (-1), disable (0), or prune after N real days |

Source: `ServerConfig.java`. Check world-specific min-Y and Corelib storage before porting.

## Call chain, persistence, and details requiring parity tests

1. `DeathEvents.playerDeath` calls `event.storeDeath()` unless history storage duration is zero, removes drops, creates the corpse from the death record and spawns it, then removes expired records. **No independent item duplication** must occur.
2. `CorpseEntity.createFromDeath` captures owner identity, name, pose/model, equipment, full inventory and death location. It clamps spawn Y to world minimum.
3. `CorpseEntity.tick` applies custom gravity/fluid buoyancy, increments age, changes to skeleton when `age >= skeleton_time`, forces removal only when `age > force_time > 0`, and removes empty corpses when `age - first_empty_age >= despawn_time`. The first-empty time is persisted and **not reset when items appear again**.
4. `CorpseEntity.interact` authorizes the corpse owner or an admin when owner-only is true; optional public skeleton access unlocks when skeleton; otherwise everyone can open. `CorpseInventoryContainer.transferItems` restores slots and handles overflow. This needs atomic server-authoritative inventory updates.
5. `CorpseEntity.remove` spills all inventory contents into the world before removal (including forced removal), then informs nearby players to render smoke. Removal reason and save/unload lifecycle should be checked to avoid dropping items during chunk unload; the original calls `remove` for all reasons.
6. `CorpseEntity.addAdditionalSaveData` stores `Death`, `Age` and optional `EmptyAge`, and reload restores synced owner, model and equipment.
7. History `/deathhistory` requires moderator privileges and supports both name and UUID arguments; the client's own request opens history without command. Historical items are recoverable only under creative-mode restrictions.

## ABI gaps / requests to WS-0

**Priority 1 — blocking:** append-only `moddedmc:player_death` interface with death callback before vanilla drops, full main/armor/offhand/additional inventory snapshot, an atomic "claim and suppress original drops" operation, and world/player references. Without this we cannot create a non-duplicating corpse. No `on_player_death`, `player_death` or player-inventory hooks were found in `api/`, `mods/` or `docs/MOD-ABI.md` at this session's branch HEAD.

**Priority 2:** custom nonliving entity spawn/save/interact with 2×0.5 collision shape; authoritative movement and despawn; correct chunk unload/reload behavior. Existing `moddedmc:mobs` v1 is living-entity carrier-only.

**Priority 3:** player inventory slot reads/writes and a transactional slot-menu API; client-visible rendering of player-skin corpse, skeleton pose and history. `moddedmc:containers` v1 addresses world block inventories, not player inventory screens.

**Available:** `moddedmc:saved_data` v1 for persistent death history; `moddedmc:commands` v1 for a limited command entry; `moddedmc:entities` v1 for generic loaded-entity view/motion/data; `moddedmc:item_stacks` v1 for world item entities; none substitutes for the death/drop API.

## Session work and verification

- Created `mods/corpse` through `scripts/new-mod.ps1`, discarded the generated example block/item/recipe/scenario, and added a policy-only crate with `rules.rs` tests for owner/admin/skeleton permissions, first-empty and forced timers, inclusive skeleton boundary, and state restoration. **It does not yet intercept death or create an entity.**
- `cargo fmt -p corpse` **PASS**; `cargo clippy -p corpse --all-targets -- -D warnings` **PASS**; `cargo test -p corpse` **PASS** (10/10); `git diff --check` **PASS**. These test pure access/lifecycle rules only, not live server behavior.
- Commit hashes and request issue numbers are recorded in the coordination-board handoff.
- **Not verified:** NeoForge original-server oracle, actual death capture, item transfers, menus, packets, entity spawning, save/reload on a real world, visuals, multiplayer permissions, world startup scenarios.
- Next session: confirm the pending ABI request and its implementation, audit Corelib `Death` storage format and original jar against the Rust data model; then implement one death-to-corpse mechanic and a bot death/restart scenario. Do not claim M2/M3 parity from these policy tests.

## Death-drop interface

`moddedmc:player_death` now provides the saved inventory stacks by slot for
one death. The host saves a successful claim before suppressing those drops.
The fixture in `tests/scenarios/abi-player-death.txt` checks acceptance,
decline, duplicate attempts, a rejected write, Curse of Vanishing,
keepInventory, and both the player inventory and claimed record after restart.

The gameplay port still uses its policy-only registration. The inventory menu
and persistent entity are tracked in GitHub issues [#22](https://github.com/devmogged/moddedMC-rust/issues/22)
and [#23](https://github.com/devmogged/moddedMC-rust/issues/23).
