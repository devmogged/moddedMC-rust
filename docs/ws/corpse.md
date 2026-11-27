# Corpse

## Release and provenance (2026-11-20)

- Upstream: [henkelmax/corpse](https://github.com/henkelmax/corpse); author Max Henkel.
- Latest published 26.3 build found: **Corpse 1.1.22+26.3**, NeoForge, released 2026-10-22, CurseForge file **9289341** ([release](https://www.curseforge.com/minecraft/mc-mods/corpse/files/9289341)).
- Verified the checked-out upstream source at `D:\mc-ref\corpse-1.1.22-26.3`: commit `12881669fb0ef95e0a863a524bc92674cc2f5608` (2026-10-22), `gradle.properties` declares `minecraft_version=26.3`, `mod_version=1.1.22+26.3`.
- Verified the *published* 252,237-byte jar, stored **outside version control** in `cache/corpse-neoforge-1.1.22+26.3.jar`: valid ZIP with 184 entries, including `CorpseMod.class`, `CorpseEntity.class`, `ServerConfig.class`, NeoForge metadata, and assets.
- **License restriction:** upstream `META-INF/neoforge.mods.toml` and the published jar explicitly declare **All rights reserved**; no separate license file was found at the source root or among inspected jar entries. All original code and assets remain outside our public repository. The owner previously reported mod-port permissions have been obtained; publication of an adaptation and redistribution of assets must remain within the actual authorization. Do not import its art or Java into our public Git history.
- Upstream core dependency: Corelib `2.1.12+26.3` at `a97e109bd134964868aa1b3561df0d071fd19c1f`. Its death event reconciles the equipment and inventory snapshot with the stacks that vanilla actually drops.

## Complete registered-content inventory from the 26.3 sources

| Kind | Upstream registrations or behavior | Host status |
| --- | --- | --- |
| Blocks | None | N/A |
| Block entities | None | N/A |
| Items | None | N/A |
| Recipes and loot tables | None | N/A |
| World generation | None | N/A |
| Entities | `corpse:corpse` (nonliving, 2×0.5, tracking distance 128, updates every tick); owner UUID/name, skeleton flag, model, equipment sync | Persistent stock carrier and sleeping equipment view |
| Menus | `corpse:corpse_inventory`, `corpse:corpse_additional_items`; transfer to matching player slots and navigate overflow pages | Stock inventory with take-only slots, matching transfer, and Additional Items controls |
| Commands | Operator-level `/deathhistory <player>` and `/deathhistory <player_uuid>`; normal history opened from client key | `/deathhistory` opens personal history on stock Java. `/corpse_recover` lets the owner recover a nearby saved body. Administrator lookup remains pending |
| Serialization | Equipment-map and UUID entity data serializers; entity NBT `Death`, `Age`, `EmptyAge` | Per-save death records and saved carrier age and empty time |
| Player death | Corelib `PlayerDeathEvent` stores death history, takes all vanilla drops, then spawns CorpseEntity | Commits the recoverable inventory before suppressing the original drops |
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

## Implemented recovery

- [Player death #17](https://github.com/devmogged/moddedMC-rust/issues/17) saves the corpse record before suppressing vanilla drops. [Native inventory menus #22](https://github.com/devmogged/moddedMC-rust/issues/22) commit changes before moving items to the player. [Native entities #23](https://github.com/devmogged/moddedMC-rust/issues/23) persist the interaction carrier and show a sleeping mannequin or skeleton on stock Java.
- A player can take their saved gear, offhand item and inventory from the body's stock chest screen. `corpse-recovery.txt` verifies one body, no loose drops, and recovery once after a cold restart. `corpse-multiple-dimensions.txt` verifies separate Overworld and Nether bodies and inventories in the same save.
- If a carrier is missing, `/corpse_recover` opens a nearby, still-present record owned by the caller in the current dimension. It uses the last saved body location. Corpses are public by default, as in the original.
- Click **Transfer items** on the hopper in the corpse screen to restore matching player slots. Displaced stacks go into player storage when space is available. Additional Items keeps the remainder with the body, including after a restart. `corpse-matching.txt` checks a helmet and shield with a full inventory and a concurrent viewer. The controlled transfer and both screens were also verified on stock Java 26.3. [Stock menu controls #33](https://github.com/devmogged/moddedMC-rust/issues/33)
- `/deathhistory` shows a player's own death dates, coordinates and original items. Survival players can inspect those items. Creative players can take a copy or use **Transfer items** when displaced items fit in player storage. `corpse-history.txt` checks viewing after a restart and rejects a second player's attempt to request the first player's saved items. `corpse-history-creative.txt` checks recovery and a switch to Survival with the preview open.
- Put the reference release's `corpse-synced.toml` in the server working directory's `config/` folder and restart. The port reads `death_storage_duration`, `skeleton_time`, `show_equipment`, `fall_into_void`, and both keys under `[corpse.despawn]`. `corpse-settings.txt` checks that disabling history leaves a visible, recoverable body and that `skeleton_time = 0` presents a skeleton immediately. `corpse-forced-despawn.txt` checks that a timed-out body spills its remaining items. Void limits currently use the built-in dimensions' bounds.
- Checked the original `1.1.22+26.3` in a NeoForge `26.3.0.37-beta` survival world. With 36 full inventory slots and an iron helmet equipped, transfer equipped the corpse's diamond helmet and shield and exposed the iron helmet in Additional Items. The operator history accepted `/deathhistory tester` and showed the original gear, death coordinates, dimension, and timestamp. The stock port uses a chest screen and a vanilla dialog for those views.
- [The Corpse port #31](https://github.com/devmogged/moddedMC-rust/issues/31) tracks the remaining settings and visual parity. [Issue #43](https://github.com/devmogged/moddedMC-rust/issues/43) covers moderator lookup and the administrator exception for owner-only access.
