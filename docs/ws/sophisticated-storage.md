# WS-19 — Sophisticated Storage (M0 survey and license gate)

Status: **source inventory and gap analysis only; no port code committed**. The upstream code and asset license blocks a distributable derivative unless permission covering the intended work is confirmed. This note records observable mod features and compatibility requirements, not licensed implementation material.

## Reference, release, dependencies and rights

- Target server: moddedMC-rust / Pumpkin for **Minecraft Java 26.3**, one independent native `.mmc` mod, with any common upgrade functionality coordinated with WS-18.
- Upstream: [P3pp3rF1y/SophisticatedStorage, `26.3` branch](https://github.com/P3pp3rF1y/SophisticatedStorage/tree/26.3), inspected tree `7db8514af81effc1e9873a284611fedcc8a75633`.
- Release reference: NeoForge Minecraft 26.3, project version `1.5.134` in [gradle.properties](https://github.com/P3pp3rF1y/SophisticatedStorage/blob/26.3/gradle.properties). The 26.3 release file is `sophisticatedstorage-26.3-1.5.134.2242.jar`, [CurseForge file 9692546](https://www.curseforge.com/minecraft/mc-mods/sophisticated-storage/files/9692546) (November 18, 2026). Recheck publisher releases before M1; a moving branch is not a pinned oracle.
- Upstream dependency bounds declare Sophisticated Core `[26.3-1.5.39,26.4)` and Sophisticated Backpacks `[26.3-3.26.21,26.4)` (the latter used for compatibility).
- **Licensing gate:** [upstream LICENSE.md](https://github.com/P3pp3rF1y/SophisticatedStorage/blob/26.3/LICENSE.md) is **All Rights Reserved** for code, art and most project files; translations are expressly unrestricted. Its text restricts publishing modifications and creating/distributing derivatives without permission. The owner's general statement of having some mod permissions does not identify this mod, license scope, derivative-port publication rights, or Sophisticated Core rights. Obtain/confirm explicit rights before code conversion or committing upstream-generated recipes, loot, models or other restricted content. This survey contains original descriptions and identifiers only, no copied source.
- Source organization: 275 Java production files, 40 test Java files and 2 disabled compatibility Java files at the inspected tree (317 total). Compare downstream behavior to the pinned released jar, not buja's older installed jar.

## Content inventory (observed registrations)

[ModBlocks.java](https://github.com/P3pp3rF1y/SophisticatedStorage/blob/26.3/src/main/java/net/p3pp3rf1y/sophisticatedstorage/init/ModBlocks.java) registers **54 fixed blocks with corresponding block items**, plus dynamically registered storage connectors for wood types:

| Family | Exact registered variants or pattern | Fixed blocks |
| --- | --- | ---: |
| Barrels | `barrel`, `{copper,iron,gold,diamond,netherite}_barrel` | 6 |
| Limited barrels | `limited_barrel_{1..4}` and `limited_{copper,iron,gold,diamond,netherite}_barrel_{1..4}` | 24 |
| Chests | `chest`, `{copper,iron,gold,diamond,netherite}_chest` | 6 |
| Tanks | `tank`, `{copper,iron,gold,diamond,netherite}_tank` | 6 |
| Shulker storage | `shulker_box`, `{copper,iron,gold,diamond,netherite}_shulker_box` | 6 |
| Linked storage | `controller`, `storage_link`, `storage_io`, `storage_input`, `storage_output` | 5 |
| Decoration | `decoration_table` | 1 |
| Wood connectors | `storage_connector` variants dynamically registered per supported wood type | variable |

[ModItems.java](https://github.com/P3pp3rF1y/SophisticatedStorage/blob/26.3/src/main/java/net/p3pp3rf1y/sophisticatedstorage/init/ModItems.java) contains **80 further item registrations**, separate from block items. Inventory by category (names resolve at the source link):
- Filter, pickup, magnet, feeding, compacting, void: basic and advanced upgrades (12).
- Smelting, smoking, blasting, each with automatic versions (6); crafting and stonecutter upgrades (2).
- Stack capacity tiers 1, 1-plus, 2–5 (6); conversions between tiers (15); downgrade tiers 1–3 (3); omega tier (1).
- Jukebox basic/advanced (2); pump/advanced and XP pump (3); compression (1); hopper/advanced (2); infinity/survival infinity (2); alchemy/advanced (2).
- Storage tier upgrades: basic + each upward tier conversion among basic/copper/iron/gold/diamond/netherite (16).
- `upgrade_base`, `packing_tape`, `super_packing_tape`, `storage_tool`, `debug_tool`, `paintbrush`, `inaccessible_slot` (7).
- Some upgrades are implemented in **Sophisticated Core** rather than this repository; names alone do not establish behavior. WS-18 and WS-19 must share one reviewed, independently buildable `mods/sophisticated-core` design, not duplicate the storage-wrapper/upgrades logic.

Ten observed block-entity registrations: barrels, limited barrels, chests, shulkers, tanks, controllers, storage links, storage IO/input and connectors (storage output shares an IO family). No independent mob/entity-type registration or entity-spawn rules identified; `entity/` Java classes wrap movable storage and openers. No new biome/worldgen/structure registry or dedicated command located in this tree; verify runtime compatibility behavior against the released jar before declaring absence conclusively.

### Inventory capacities and configuration

[Config.java](https://github.com/P3pp3rF1y/SophisticatedStorage/blob/26.3/src/main/java/net/p3pp3rf1y/sophisticatedstorage/Config.java) declares these defaults, in **wood/copper/iron/gold/diamond/netherite** order:

| Type | Values by tier |
| --- | --- |
| Chest, barrel, shulker inventory slots | 27 / 45 / 54 / 81 / 108 / 132 |
| Upgrade slots | 1 / 1 / 2 / 3 / 4 / 5 |
| Limited barrel type 1 capacity | 32 / 53 / 64 / 96 / 128 / 160 |
| Limited barrel type 2 capacity | 16 / 27 / 32 / 48 / 64 / 80 |
| Limited barrel type 3 capacity | 10 / 17 / 20 / 30 / 40 / 50 |
| Limited barrel type 4 capacity | 8 / 13 / 16 / 24 / 32 / 40 |
| Tank capacity (millibuckets) | 8,000 / 12,000 / 16,000 / 32,000 / 64,000 / 128,000 |

Key settings include `dropPacked` (common, default false), configurable per-storage upgrade limits, disallowed shulker-box items, controller linking range (default 15; allowed 4–64), excessive entity drop threshold (default 200), limited-barrel count-dyeing (enabled), and filtering of externally registered wood types. Client settings govern creative-item variant visibility. A correct port needs persistence and reconfiguration semantics, not only these defaults.

### Recipes, loot, menus, networking, client features

At the pinned tree, generated data contains **329 recipe JSON files, 326 recipe advancements, 66 loot tables, 12 tag files** (3 mod, 6 common, 3 Minecraft) under `src/generated/resources/data`. Counts describe source files, not distinct behaviors proven on our server. Content requires license verification before copying. [ModBlocks.java](https://github.com/P3pp3rF1y/SophisticatedStorage/blob/26.3/src/main/java/net/p3pp3rf1y/sophisticatedstorage/init/ModBlocks.java) registers **10 custom recipe serializers** (dye, tier upgrade and double-chest variants, shapeless upgrade variants, vanilla/other chest to shulker, generic wood storage, flat-top barrel toggle and barrel material), and two custom ingredient types (`base_tier_wooden_storage`, `non_sophisticated_chest`).

Observed **5 menu type registrations**: storage, tank, limited barrel, settings and decoration table; sub-screen Java classes cover further settings and side IO. The original mod uses storage-specific inventory interactions, filters, slots, automation, locking, item count displays, packing, dyeing, tier upgrade in place, linked storage networks, tank fluids and controllers. These must be oracle-tested individually, with NBT and restart round trips.

[ModPayloads.java](https://github.com/P3pp3rF1y/SophisticatedStorage/blob/26.3/src/main/java/net/p3pp3rf1y/sophisticatedstorage/init/ModPayloads.java) declares **8 custom play payloads**: serverbound open-storage, request-contents, scrolled-tool, request-player-settings, request-controller-highlights; clientbound storage-contents, tank-menu-update and storage-openness. Multiple screens/renderers are client-side. Stock Java 26.3 clients cannot be assumed to display the original NeoForge menu protocol; coordinate data-driven menus and visuals with WS-8 companion.

Compatibility integration source covers recipe viewers (JEI, REI/EMI), Jade, Quark, Chipped, Sawmill, Sophisticated Backpacks, Tom's Storage and TrashSlot. Inventory completeness requires testing these integrations when corresponding mods are in the pack.

## ABI mapping (checked against main `docs/MOD-ABI.md`)

| Feature | Current host interface | Status/next question |
| --- | --- | --- |
| Declare standalone blocks and item definitions | `moddedmc:content` v1 and `moddedmc:items` v1 | **Exists** for static definitions and stand-in models |
| Persist block-entity metadata and read/write blocks | `moddedmc:block_world` v1, block-entity NBT | **Exists** for host-visible state and NBT; verify storage-wrapper serialization |
| Per-world persistent controller registry | `moddedmc:saved_data` v1 | **Exists** for global NBT; implement controller linking in mod |
| Read and transfer to vanilla containers | `moddedmc:containers` v1 | **Partly exists** (vanilla item handler slots/insert/extract); verify dynamic *mod-owned* storage capability, sided wrapper, upgrade transforms and transactional behavior |
| Register ordinary datapack recipes, loot and tags | built-in `.mmc` `data/` loading | **Exists for supported vanilla types only**; custom 10 recipe serializers / ingredient predicates need a host service or conversion with evidence |
| Interactive storage/inventory/tank menus | `Player::show_menu` basic dialog and `menu_click` | **Missing parity** for arbitrary per-slot inventory UI, upgrade/filter configuration and bidirectional payloads; WS-0/WS-8 |
| Tank fluid capabilities, linking, sided transfer | `moddedmc:block_world` and existing fluid-state inspection | **Not established** as a fluid-tank transaction interface; compare WS-2 fluid requests before filing new ABI request |
| Item-specific actions and tier upgrades in place | `moddedmc:items`, `moddedmc:block_world`, interaction callbacks | **Partly exists**; atomic inventory/NBT migration and item-component hooks need confirmation |
| Network/controller-backed inventories | `moddedmc:saved_data`, `moddedmc:containers` | **Partly exists**; require exact cross-chunk loading, topology, side access and conservation tests |
| Client-only visual features | server resource pack plus companion plan | **Incomplete** (dynamic wood styling, lock/count overlays, tank renderer, full GUIs) |
| Custom commands or custom mobs | no required registrations identified | **No scope observed** in inspected registry source |

No ABI request filed yet: first compare the latest `origin/ws/abi` and existing open `devmogged/mmc-coord` requests to prevent duplicates. A missing service must be formally requested via `coord.mjs request`; this workstream must not edit `api/` or the host.

## Verification, status, and next session

- **PASS (source/document audit):** inspected upstream 26.3 branch tree, release metadata, `LICENSE.md`, registry classes, config, payloads and generated resource paths. Counted fixed registrations and data assets from the source tree.
- **PASS (workspace safety):** worktree `N:\mmc-wt\ws-19` created on `ws/sophisticated-storage` from `origin/main`; no foreign changes present at start.
- **NOT RUN:** `cargo fmt`, Clippy, unit tests, bot scenarios, persistence test, original NeoForge oracle, performance measurements. No Rust crate or source/data port exists to test. M0 survey is not M1 implementation or parity evidence.
- **HOLD:** written grant covering publication of a Rust derivative of both Sophisticated Storage and relevant Sophisticated Core code/data must be confirmed, with any deployment restrictions documented. The upstream's explicit ARR license is not superseded by a generic permission statement concerning other mods.
- **NEXT (after license resolution):** coordinate `mods/sophisticated-core` APIs with WS-18; pin actual release jar outside git; resolve ABI/menu/fluids gaps with WS-0/WS-8; add a minimal authorized crate, test one basic storage block's inventory insertion, extraction and restart persistence against the original NeoForge jar. Compare default capacities, conservation, NBT, UI and saved controller links before expanding tiers/upgrades.
