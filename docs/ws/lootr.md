# WS-13 Lootr: source survey and port status

## Release and rights (M0)

- **Target**: Lootr **1.25.42.124**, Minecraft Java **26.3**, latest published Fabric/NeoForge release checked 2026-11-20. Modrinth version `gVwn7m2v`: https://modrinth.com/project/EltpO5cN/version/gVwn7m2v
- **Oracle source**: `D:\mc-ref\lootr-1.25.42.124` at `2a9c8307460e3ff5765ecd849b11a6faf06dfe8b` (release commit on upstream `mdg-26.3`). GitHub: https://github.com/LootrMinecraft/Lootr/tree/2a9c8307460e3ff5765ecd849b11a6faf06dfe8b
- **Released Fabric JAR**: ignored `cache/lootr-fabric-26.3-1.25.42.124.jar` (1,583,342 bytes); SHA-512 `290786fd4af4011c4f09c425d262bacc80f6ef47e23fbf6424193c719b6045a70cae70308c95d30c5ab78bf1f5a3d089cc3b01db8cc4dacc1a74c0f2d70c245d` matches Modrinth published hash.
- **Code**: upstream `LICENSE` is MIT. Preserve attribution with copied MIT notice in the crate. **All assets** are All Rights Reserved according to upstream `LICENSE_ASSETS.txt`; do not copy `assets/` to this repository or distribute them in `.mmc`. Runtime asset lookup may use the owner's original JAR.
- **JAR inventory**: 15 `data/lootr/loot_table/` files, 73 `data/lootr/tags/` files, 21 `data/lootr/advancement/` files, **zero recipes**, 124 `assets/lootr/` files (not redistributable). Other namespaces' tags also ship with the original JAR.

## Feature inventory, from release source

| Kind | Complete registered inventory | Java entry point |
| --- | --- | --- |
| Blocks (12) | `barrel`, `chest`, `trapped_chest`, `shulker_box`, `copper_chest`, `exposed_copper_chest`, `weathered_copper_chest`, `oxidized_copper_chest`, `suspicious_sand`, `suspicious_gravel`, `decorated_pot`, `trophy` | `neoforge/.../init/ModBlocks.java` |
| Items (12) | The matching 12 block items; `trophy` is EPIC rarity. No separately registered standalone items | `neoforge/.../init/ModItems.java` |
| Block entity types (6) | `chest`, `trapped_chest`, `barrel`, `shulker_box`, `brushable_block` (both substrates), `decorated_pot` | `common/api/LootrConstants.java`, `common/block/entity/` |
| Entity types (2) | `chest_minecart` (0.98 × 0.7), `item_frame` (0.5 × 0.5) | `neoforge/.../init/ModEntities.java`, `common/entity/` |
| Inventory and menu | Per-player per-container inventories, generated from the original vanilla loot table on access, persisted independently; chest/barrel/shulker/cart inventory screens and open/close tracking; item frame and pot special handling | `common/data/{DataStorage,LootrInventory,LootrInventoryStore,Section}.java`, `common/api/data/`, `common/block/entity/` |
| World/structures | Convert eligible structure loot containers and frames/carts; dimensions/loot table allow/deny filters; tick-delayed processing, saved chunk/section state | `common/impl/conversion/`, `common/chunk/LoadedChunks.java`, `common/mixin/structure_saving/` |
| Break/redstone | Configurable break modes, protections, explosion resistance, break drops, trapped-chest power and comparator outputs | `common/api/config/LootrCommonConfig.java`, `common/block/`, `common/mixin/` |
| Refresh/decay | Per-container/per-loot-table refresh and decay triggers and delays, optional replacement and particle notifications | `common/api/config/LootrCommonConfig.java`, `common/data/` |
| Loot/data | 15 loot tables: 12 block drops plus `chests/elytra`, `entity/item_frame_empty`, `reward/trophy`; 73 Lootr tags; 21 advancements; zero recipes | `common/src/generated/resources/data/` |
| Client | Custom block, entity, screen renderers, HUD/toasts, refresh and unopened particles; server packets below; all upstream visual assets are ARR | `common/client/`, `fabric/network/` |

**Commands** registered by `common/command/CommandLootr.java`: `/lootr` root, `clear <profile>`, `open_as <profile>`, `open_as_uuid <uuid>`, `id`, `refresh`, `decay`, `openers <location>`, `custom-map <level>`, `custom-chest <target>`, `custom-area-old <from> <to>`, `cclear <entities>`, `force_chunk`, `force_radius <radius>`, `force_all`, plus registered extension commands. This is source registration inventory, not a claim that the Rust port implements them.

**Network**: Fabric's 9 server-to-client payload types `PacketAreaEntitySync`, `PacketCloseCart`, `PacketCloseContainer`, `PacketContainerStatus`, `PacketOpenCart`, `PacketOpenContainer`, `PacketPerformBreakEffect`, `PacketRefreshSection`, `PacketSyncConfig`; one client-to-server payload `PacketRequestUpdate`. NeoForge includes the equivalent handlers.

**Common config keys** (source `LootrCommonConfig.java`):

- Conversion: `disable`, `save_mode`, `randomise_seed`, `convert_elytras_to_chests`, `convert_elytras_to_item_frames`, `convert_structure_item_frames`, `check_world_border`, `perform_piecewise_check`, `tick_delay`.
- Restrictions: `dimension_whitelist`, `dimension_blacklist`, `modid_dimension_whitelist`, `modid_dimension_blacklist`, `loot_table_blacklist`, `loot_table_modid_blacklist`, `loot_table_force_whitelist`.
- Breaking: `break_mode`, `enable_fake_player_break`, `blast_resistance`, `brushables_self_support`, `item_frames_self_support`, `should_drop_player_loot`, `break_to_drop_loot`.
- Redstone: `power_comparators`, `custom_trapped`.
- Notifications: `report_unresolved_tables`, `disable_message_styles`.
- Interaction: `bypass_spawn_protection`.
- Decay: `decay_value`, `decay_loot_tables`, `decay_loot_table_modids`, `decay_dimensions`, `replace_when_decayed`, `perform_decay_while_ticking`, `start_decay_while_ticking`, `decay_all`.
- Refresh: `refresh_value`, `refresh_loot_tables`, `refresh_loot_table_modids`, `refresh_dimensions`, `perform_refresh_while_ticking`, `start_refresh_while_ticking`, `refresh_all`.
- Team: `team_loot`, `pinned_team_resolver`.
- Client `LootrClientConfig.java`: `unopened_particles`, `refresh_particles`, `decay_particles`, `vanilla_textures`, `display_toast`, `disable_notifications`, `maximum_notification_delay`.
- Enums: `BreakMode = DEFAULT | NEVER | ALWAYS`, `SaveMode = ALWAYS | SMART | WHEN_OPENED` (invalid string defaults to SMART); `ResistanceMode` in its own source file.

## Host capability map

| Needed by Lootr | Current ABI | Status/next step |
| --- | --- | --- |
| Register custom block states, shape, placement, simple drops, item IDs | `moddedmc:content` v1; `moddedmc:items` v1; built-in `data/` | **Exists** for data/simple content. Physical template does not express arbitrary properties like trophy's exact light=15/strength=15. |
| Read/write custom block entities and loaded blocks, receive block events | `moddedmc:block_world` v1; `moddedmc:content` v1 | **Exists**, but not a full vanilla container lifecycle hook. |
| Observe interactions and accepted player breaks | `moddedmc:interaction` v1, including `BlockUse`, `PlayerWillDestroy` | **Exists** for Lootr's own blocks. Generic interception of arbitrary pre-conversion vanilla blocks is not described. |
| Store per-player records across save/restart | `moddedmc:saved_data` v1 | **Exists** for opaque global bytes; needs a port of Lootr's record model, UUID and snapshot semantics. |
| Read vanilla world loot tables | `moddedmc:resources` v1 | **Partial**: JSON read/list exists; no documented host-level vanilla loot-table generation with LootContext/seed, so exact vanilla loot rolling is **missing**. |
| Open vanilla inventory containers per player and synchronize slot moves | Base `Player::show_menu` and `Mod::menu_click` cover custom button menus only; `moddedmc:containers` v1 is a vanilla block inventory item-handler | **Missing** generic inventory-menu open/change/close host interface for independent per-player Lootr inventory. |
| Convert generated vanilla structure chests/carts/pots/brushable blocks | `moddedmc:block_world` and `moddedmc:content` | **Partial**: generic block writes exist, but no hook for generated vanilla structure/container creation with original loot-table/seed context. |
| Register arbitrary chest minecart and hanging item frame entity types, preserve their state | `moddedmc:mobs` v1 is for living carriers; `moddedmc:entities` v1 can view/change existing | **Missing** generic nonliving custom entity registration/lifecycle. |
| Comparator, trapped-chest power and vanilla openers | `moddedmc:block_world` exposes neighbor signal reads, not custom outgoing comparator/trapped power | **Missing** output hook/interface. |
| Particle/sounds, resource tags, commands | `moddedmc:effects` v1, `moddedmc:tags` v1, `moddedmc:commands` v1 | **Exists** for basic effects/tags/command wiring. Custom packet transport / client renderer integration requires separate design. |

## Implementation and verification ledger

- **M0 source/license survey**: reviewed pinned release source and matching SHA-512 verified JAR. Survey commit `5f64a59` pushed to `ws/lootr`. Original Java gameplay oracle **not run**.
- **M1 partial content**: `mods/lootr` native `cdylib` registers `lootr:trophy` with four exact `facing` selectors, placement direction opposite yaw, approximate shape, orientation, and original `data/lootr/loot_table/blocks/trophy.json`. No original assets redistributed; resource pack reads the ignored source JAR from `cache/`. `Cargo.toml` version `1.25.42+124` preserves upstream release build number in SemVer metadata. The released mod registers **12** blocks; **only trophy** is currently registered. There are no upstream recipes. No false chest inventory mechanic is registered.
- **M1 verification**: `cargo fmt --all --check` PASS; `cargo test -p lootr --all-targets` **7/7 PASS** (2 trophy placement-yaw, 5 inventory-store domain tests); `cargo clippy -p lootr --all-targets -- -D warnings` PASS; `scripts/pack-mod.ps1 -Crate lootr` PASS on Windows x86-64. `tests/scenarios/lootr-trophy.txt` exercises block registration, initial facing, trophy loot table, giving item, saved block state through full stop/restart, and no server errors. Run with shared binary: `scripts/scenario.ps1 -Scenario tests/scenarios/lootr-trophy.txt -RunDir run/ws-13-lootr-trophy -Port 26130 -Fresh -Exe '..\\..\\mmc-shared\\bin\\9d9e63f924-e1b1f49\\pack-dev.exe' -BotExe '..\\..\\mmc-shared\\bin\\9d9e63f924-e1b1f49\\moddedmc-bot.exe'`.
- **M2 mod-side model (not live)**: `mods/lootr/src/inventory_store.rs` ports `LootrInventoryStore.java` ownership by resolved player/team UUID, reuse without re-rolling, visual versus actual opener tracking, refresh clearing, individual clear semantics, game-time decay/refresh sentinels and deadlines. Five deterministic unit tests PASS. It is **not wired to world callbacks, saved-data persistence or menus**, and no live per-player loot scenario is claimed.
- **Host dependencies**: issue [#74](https://github.com/devmogged/mmc-coord/issues/74) requests compiled vanilla chest `LootTable.fill` access with player/luck/origin/seed; existing [#61](https://github.com/devmogged/mmc-coord/issues/61) covers generic inventory menus; [#63](https://github.com/devmogged/mmc-coord/issues/63) covers per-block physical overrides. Additional generated-structure conversion and nonliving entity registration contracts remain to be designed. Do not invent the missing host functionality in this mod.
- **Known differences**: upstream `TrophyBlock.java` uses EAST_WEST (1.5,0,4)-(14.5,14.5,12), NORTH_SOUTH (4,0,1.5)-(12,14.5,14.5); current C ABI `ShapeBox` supports *integer* sixteenths only, so the port rounds to integer bounds. Upstream trophy emits light 15, hardness 15 and metal sound; its temporary `minecraft:stone` physical template does not reproduce these until host request #63 is answered. Client model/asset oracle is **not run**; server warns about a missing content-pack asset placeholder in the local scenario even when detecting the source JAR.
- **Next**: first wire real per-player inventory open/mutation/close to WS-0's generic menus when available; bridge the Rust model through `moddedmc:saved_data` and issue #74's real loot-table evaluator; add multi-player open/close/refresh/restart differential against the released Java Lootr jar. Separately implement the remaining 11 block IDs with exact source gameplay and conversion, then chest minecart/item-frame entities. Validate client model orientation/ARR resource-pack fallback and record actual visual parity, not just a successful build.
