# WS-11 — Waystones source survey

Status: **M0 source inventory in progress; implementation gated by upstream redistribution rights**.
Workstream: `WS-11`, branch `ws/waystones`. No Rust crate or Java-derived code was published in this survey.

## Version and provenance (verified 2026-11-20)

- Target: **Waystones 26.3.0.3** for Minecraft Java **26.3**, NeoForge and Fabric, published **2026-11-15**. Official NeoForge file: https://www.curseforge.com/minecraft/mc-mods/waystones/files/9626191 ; Fabric file: https://www.curseforge.com/minecraft/mc-mods/waystones/files/9626195 .
- Official source: https://github.com/TwelveIterations/Waystones/tree/26.3 ; inspected tree `3e1e785d29f176ee83fc157c1bac32320c55d386`. Its CHANGELOG matches the November 15 release notes. The release artifact itself was **not** downloaded or executed, so jar/source parity remains unverified.
- Dependencies: Balm; in Minecraft 26.1+ **Shogi** supplies rule evaluation and cooldown/requirement effects. Upstream description: https://github.com/TwelveIterations/Waystones/blob/26.3/DESCRIPTION.md .
- **Rights gate:** https://github.com/TwelveIterations/Waystones/blob/26.3/LICENSE says **All Rights Reserved**. The publisher's https://mods.twelveiterations.com/permissions (2026-11-05) allows private modified copies shared within a community, but explicitly disallows public republishing of mod files or forks. There is no documented public-derivative grant in those materials. The target `devmogged/moddedMC-rust` is public. Before publishing a source-derived port or copied recipes/assets, the owner must verify an express grant that covers this new Rust derivative and public redistribution, or have the plan amended to permit a clean-room, original implementation with a documented legal boundary. **Do not construe modpack distribution permission as permission to republish a fork.** All visual, audio, model and language resources remain out of this repository regardless; two item icons have a separate CC-BY-SA 3.0 notice (`LICENSE-icons`).
- Owner decision needed: written permission scope or approved clean-room strategy. Until then, do not copy source implementation, recipes, resources, or put ported code in the public repository.

## Content inventory from the 26.3 source tree

All identifiers below are `waystones:` names. Counts are generated-resource entries, **not** proof that every entry is obtainable in Survival.

**31 blockstates**:
- 10 waystones: `andesite_waystone`, `blackstone_waystone`, `deepslate_waystone`, `end_stone_waystone`, `mossy_andesite_waystone`, `mud_bricks_waystone`, `prismarine_waystone`, `purpur_waystone`, `red_nether_bricks_waystone`, `sandstone_waystone`.
- 9 portstones: `portstone`, and `amethyst`, `copper`, `diamond`, `emerald`, `gold`, `lapis`, `prismarine`, `redstone` `_portstone`.
- 9 sharestones: `amethyst`, `copper`, `diamond`, `emerald`, `gold`, `lapis`, `prismarine`, `redstone`, `ruined` `_sharestone`.
- Other three: `warp_plate`, `warp_portal`, `fleeting_memorial`. The last two have no ordinary block drop in the source registration.

**48 generated item model entries**, comprising the 29 block item forms (31 blocks less `warp_portal` and `fleeting_memorial`), 9 warp stones (`warp_stone` plus 8 mineral/redstone variants), and 10 other content items: `attuned_shard`, `blank_scroll`, `bound_scroll`, `crumbling_attuned_shard`, `dormant_shard`, `epitaph`, `portal_scroll`, `return_scroll`, `twinbound_feather`, `warp_scroll`. Check which block item models are present separately; `epitaph` is nonstackable in `ModItems`.

**Data**: 54 generated recipe JSON files, 29 block loot JSON files, 58 advancement JSON files, and many block, item, biome, and modifier tags. Some recipes convert ruined sharestones and mossy variants. Generated tags distinguish scroll types, bound scrolls, warp stones/shards, teleport targets, and **13** `warp_modifiers` subcategory tags.

**Block entities**: `WaystoneBlockEntity`, `PortstoneBlockEntity`, `SharestoneBlockEntity`, `WarpPlateBlockEntity`, `WarpPortalBlockEntity`, `FleetingMemorialBlockEntity` plus shared base. Need saved identity, dimension/position, visibility, owner, modifier inventory, linked target, activation and independent uniqueness when structures are copied.

**Menus**: ten registered menu types: `waystone_selection`, `warp_scroll_selection`, `portal_scroll_selection`, `warp_stone_selection`, `portstone_selection`, `inventory_selection`, `admin_selection`, `sharestone_selection`, `waystone_modifiers`, `waystone` (editing). Client screens additionally manage groups, favorites/visibility, waystone ordering, personal settings and modifier editing.

**Network messages**: 12 serverbound packets (selection, editing, settings, management and sorting of waystones/groups, inventory button, modifiers) and 7 clientbound packets (known waystones, groups, sorting, updates/removals, teleport effects, epitaph activation). Source `ModNetworking` declares network version `5`. Stock-client compatibility requires alternative server-side interactions; vanilla clients cannot decode these custom mod packets or render the original screens.

**Commands**: a `/waystones` tree with permission-protected operations for activating, forgetting, counting, placing, listing, GUI access, cooldowns and twinbound links; inspect `ModCommands`, `CountWaystonesCommand` and `OpenPlayerWaystonesGuiCommand` fully before implementing grammar.

**Worldgen**: 10 distinct waystone features + 10 placed features (default, mossy, sandy, blackstone, red nether bricks, deepslate, end stone, purpur, prismarine, mud bricks). Five village Jigsaw template pools (desert, plains, savanna, snowy, taiga); wild-waystone biome/dimension predicates, village spawn frequency, configurable spacing and naming modes. Must follow E2 host worldgen after that ABI is ready.

**No standalone mob type registered** was identified in the ordinary Waystones Java source tree. Teleportation does handle vanilla pets, passengers and leashed entities; these are not new mob definitions.

## Mechanics and persistence that require exact parity tests

- Save-global waystone records are represented by `SavedDataWaystonesStore` (`Waystones` list); `WaystoneIndexManager` maintains global/team indexes and `PlayerWaystoneManager` manages discovery and activation. Per-player data, groups, favorites, ordering, visibility, ownership, transient waystones and invalid/missing dimension states need coverage.
- Source/target validity checks happen before and after async destination chunk loading; pending requests can fail when the actor moved, source item vanished, target is invalid/unloaded or the dimension disappeared. Player teleport and attached mob/passenger movement are distinct operations; warp plates have a reentry cooldown.
- **Default Shogi requirements** from source config: distance-dependent XP, configurable min/max and cross-dimensional caps, warp-stone durability, inventory-button cooldown. Defaults include XP rate 0.01 per block, max XP points cost 27, interdimensional cost 27 and inventory-button cooldown 300 s. Do not transplant a simplistic levels-only rule: the current system is rule-driven, configurable and has source-specific exceptions.
- Default visibility on activation is `activation`; optional `team` and `global`. By default pets are disabled; leashed mob transport is enabled. Warp requirements can be disabled or overridden individually via configuration. Worldgen defaults include wild chunks spacing 25 and village spawn frequency `regular`.
- Latest changelog differences must be covered: custom groups capped at 64; structure-loaded plates can use relative targets; structure-copied waystones regenerate identities; owners reset on deactivation; backing index updates on dimension/position changes; top-half auxiliary targets; modifier cost/predicate; resource-pack rune definitions and network revision.
- Storage golden tests should compare source records and restart/reload, not merely encode/decode with one custom codec. Owner-authorized original server oracle required for definitive parity.

## Host interface map (docs/MOD-ABI.md on 2026-11-20)

| Feature | Current host interface | Status |
| --- | --- | --- |
| Register items, blocks, states | `moddedmc:content` v1; `moddedmc:items` v1 | Exists (visuals on stock client require carriers) |
| Block entities and state updates | `moddedmc:block_world` v1 | Exists; verify block entity NBT scope |
| Activation, using scrolls, placement, breaking | `moddedmc:interaction` v1 | Exists |
| Hold scroll-bound UUID/destination | `moddedmc:interaction` stack NBT; `moddedmc:item_stacks` v1 | Exists, serialization needs tests |
| Store global waystone registry | `moddedmc:saved_data` v1 | Exists (8 MiB/key), independent of chunks |
| Registered recipes and tags | `moddedmc:resources` v1; `moddedmc:tags` v1 | Exists for resource access, requires licensing approval before copying data |
| World positions, dimensions, teleport | Player base ABI `teleport`; `moddedmc:world` v1 | Exists for players; full attached-entity parity unverified |
| Scroll/warp effects and cooldown | `moddedmc:effects` v1; `moddedmc:interaction` cooldown | Exists in part; XP accounting and Shogi semantic rules unverified |
| Commands | `moddedmc:commands` v1 | Exists; exact permission and argument expressivity needs inspection |
| Menu screens/modifier inventory | legacy `Player::show_menu`; WS-0 M4 menus | **Incomplete**; host menu expansion needed |
| Wild features and village structures | E2 worldgen scope | **Pending** |
| Vanilla pet, leash, passenger teleport and cross-dimension batch | `moddedmc:entities` and player base | **Gap to verify**; do not claim supported |
| Team index integration / scoreboard lifecycle hooks | no verified named table | **Gap to verify** |

No ABI requests were filed in this rights-gated survey. Once authorized, file narrowly scoped requests for confirmed missing operations, rather than building a workaround.

## Next session (after rights clearance)

1. Confirm the written grant covers a source-derived Waystones implementation pushed to the **public** repository (and data/assets separately), or obtain explicit approval for original clean-room implementation without copying protected expression. If not cleared, leave this workstream blocked.
2. Download the official **26.3.0.3** jar into ignored `cache/`, match hashes/metadata to the official release, and place its matching source at `D:\mc-ref\waystones-26.3.0.3` outside the repo. Inspect live jar for all inventories (this log uses the public `26.3` source tree only).
3. Inspect `scripts/new-mod.ps1`, create `mods/waystones`, populate crate description with `26.3.0.3` and use `moddedmc_api` as the stable ABI only. Work first on deterministic independent data structures and save-global records; write tests for duplicates, owner/team filtering, activation/deactivation, dimension updates, 64-group cap and malformed saved bytes.
4. Implement one content slice with a bot placement/break/restart scenario and a source oracle, then commit/push/submit under the coordinator. Do not mark parity from build success alone.
