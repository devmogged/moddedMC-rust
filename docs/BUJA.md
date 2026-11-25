# Buja pack inventory

Owner has confirmed the complete buja modpack is in scope, with every mod targeted at its newest published release. **Path reconciliation pending:** the supplied `D:\buja_server` path is absent on the connected laptop; the existing Forge 1.20.1 server is at `D:\minecraft\buja_server`. Its `mods/` folder contains 65 JARs, and its `mods.zip` archive contains the **same 65 unique JAR basenames**, with no additions or omissions. This is the working snapshot until an authoritative alternate instance is identified.
Snapshot: 2026-11-20. The unrelated `temp.py` is not a mod and is excluded.
No Java jars, game assets or private server files are stored in this repository.

## Reading the table

- `Bundled` is the actual file version, taken from the hash-linked release or embedded metadata and file names.
- `Latest` means newest published stable release found in the cited Modrinth project or CurseForge page on 2026-11-20, **not** the latest compatible with the old Forge 1.20.1 pack.
- 48 installed files matched an exact Modrinth SHA-1. Eleven others were matched by project name/mod ID and need publisher confirmation before porting. The six remaining were checked on CurseForge; Chunk-Pregenerator was deliberately resolved there rather than against an unrelated Modrinth datapack with the same name.
- Code licenses are transcribed from the installed JAR metadata; some use nonstandard labels and need review of the actual repository LICENSE. An open-source code license does **not** automatically grant permission to redistribute textures, models, translations or sounds. `Unknown (withhold assets)` is a blocker on copying assets.
- `port` means a server-side Rust port is needed; size is an initial S/M/L/XL estimate. `vanilla` means the functionality belongs in the engine or is already replaced by a native utility; `client` stays client-side; `library` needs only the behavior its dependants use; `datapack` waits for runtime data/worldgen or the consuming recipe machinery.

## Full inventory

| Mod | ID | Bundled | Latest (MC; loader) | Declared code license | Asset license | Required deps (excluding Minecraft/loader) | Category | Scope | Owner | Source check |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Alex's Caves | `alexscaves` | `2.0.2` | [2.0.2](https://modrinth.com/mod/alexs-caves) (1.20.1; forge, neoforge) | GNU LESSER GENERAL PUBLIC LICENSE | Unknown (withhold assets) | citadel | port (XL) | Cave biomes, mobs, bosses and equipment | WS-1 future | Modrinth exact file hash |
| Alex's Mobs | `alexsmobs` | `1.22.9` | [1.22.9](https://modrinth.com/mod/alexs-mobs) (1.20.1; forge, neoforge) | GNU LESSER GENERAL PUBLIC LICENSE | Unknown (withhold assets) | citadel | port (XL) | Animals and monsters with custom AI | WS-1 future | Modrinth exact file hash |
| AppleSkin | `appleskin` | `2.5.1+mc1.20.1` | [3.0.10+mc26.3](https://modrinth.com/mod/appleskin) (26.3; fabric) | The Unlicense | Unknown (withhold assets) | None listed | client | Food and saturation HUD | WS-8 if needed | Modrinth exact file hash |
| Architectury API | `architectury` | `9.2.14+forge` | [22.0.6+neoforge](https://modrinth.com/mod/architectury-api) (26.3; neoforge) | GNU LGPLv3 | Unknown (withhold assets) | None listed | library | Shared mod APIs | dependent ports | Modrinth exact file hash |
| Balm | `balm` | `7.3.28` | [21.0.67+neoforge-1.21.1](https://modrinth.com/mod/balm) (1.21.1; neoforge) | ARR | ARR (until independently cleared) | None listed | library | Shared APIs used by Waystones and Easy Villagers | dependent ports | Modrinth project match |
| Canary | `canary` | `mc1.20.1-0.3.3` | [mc1.20.4-0.3.3](https://modrinth.com/mod/canary) (1.20.4; forge) | LGPL-3.0 | Unknown (withhold assets) | None listed | vanilla | Vanilla server performance optimizations | engine | Modrinth exact file hash |
| Carbon Config | `carbonconfig` | `1.2.8.2` | [2.0.3](https://modrinth.com/mod/carbon-config) (1.21.1; neoforge) | Apache-2.0 | Unknown (withhold assets) | None listed | library | Configuration framework | dependent ports | Modrinth exact file hash |
| Chunk-Pregenerator | `chunkpregen` | `4.4.5.1` | [4.5.4](https://www.curseforge.com/minecraft/mc-mods/chunkpregenerator) (1.21.1; Forge, NeoForge) | ARR | ARR (until independently cleared) | carbonconfig | vanilla | Server world pre-generation utility; Chunky counterpart exists | engine / Chunky | CurseForge |
| Chunk Sending | `chunksending` | `2.8` | [4.1.1](https://www.curseforge.com/minecraft/mc-mods/chunk-sending-forge-fabric) (26.3; Fabric (NeoForge: 4.1)) | ARR | ARR (until independently cleared) | cupboard | vanilla | Chunk send scheduling and network limits | engine | CurseForge |
| Chunky | `chunky` | `1.3.146` | [1.5.66](https://modrinth.com/mod/chunky) (26.1.2, 26.2, 26.3 (+2); fabric) | GNU GPLv3 | Unknown (withhold assets) | None listed | port (M) | Chunk pre-generation commands; native port already exists | existing mods/chunky | Modrinth exact file hash |
| Citadel | `citadel` | `2.6.3` | [2.7.1](https://modrinth.com/mod/citadel) (1.21.7, 1.21.8, 1.21.9 (+9); neoforge) | GNU LESSER GENERAL PUBLIC LICENSE | Unknown (withhold assets) | None listed | library | Support for animated mobs and models | companion / mobs | Modrinth exact file hash |
| Cloth Config API | `cloth_config` | `11.1.136+forge` | [26.3.159+neoforge](https://modrinth.com/mod/cloth-config) (26.3; neoforge) | GNU LGPLv3 | Unknown (withhold assets) | None listed | library | Configuration screens and framework | dependent ports | Modrinth exact file hash |
| Connectivity | `connectivity` | `7.6` | [7.7](https://www.curseforge.com/minecraft/mc-mods/connectivity) (26.3; Fabric, NeoForge) | ARR | ARR (until independently cleared) | cupboard | vanilla | Connection timeouts and network diagnostics | engine | CurseForge |
| Corpse | `corpse` | `1.0.23` | [neoforge-1.1.22+26.3](https://modrinth.com/mod/corpse) (26.3; neoforge) | ARR | ARR (until independently cleared) | None listed | port (M) | Persistent death corpse with stored items | WS-1 candidate | Modrinth project match |
| Create More Recipes | `cmr` | `1.2.1-fix1` | [1.2.1-fix1](https://www.curseforge.com/minecraft/mc-mods/create-morerecipes) (1.21.1; NeoForge) | MIT | Unknown (withhold assets) | create | port (M) | Extra processing types and custom fan and spout recipes | WS-4 | CurseForge |
| Create | `create` | `mc1.20.1-6.0.8` | [6.0.10+mc1.21.1](https://modrinth.com/mod/create) (1.21.1; neoforge) | MIT | ARR (separate asset licence) | flywheel, ponder | port (XL) | Mechanical power, factories and moving contraptions | integrator | Modrinth exact file hash |
| Create: New Age | `create_new_age` | `1.1.7f+mc1.20.1` | [1.2.0+mc1.21.1](https://modrinth.com/mod/create-new-age) (1.21.1; neoforge) | BSD-3-Clause | Unknown (withhold assets) | create, esl | port (L) | Create power generation and energy devices | WS-4 | Modrinth exact file hash |
| Create: Central Kitchen | `create_central_kitchen` | `1.4.3b` | [2.6.2b](https://modrinth.com/mod/create-central-kitchen) (1.21.1; neoforge) | MIT | Unknown (withhold assets) | create | port (M) | Create processing integration with food recipes | WS-4 | Modrinth exact file hash |
| Create: Copper and Zinc | `create_copper_and_zinc` | `2.0.0` | [2.0.0](https://modrinth.com/mod/create-copper-zinc) (1.21, 1.21.1; forge, neoforge) | ARR | ARR (until independently cleared) | None listed | port (S) | Create materials and recipes | WS-4 | Modrinth exact file hash |
| Create: Enchantment Industry | `create_enchantment_industry` | `1.3.3` | [1.4.1](https://modrinth.com/mod/create-enchantment-industry) (1.20.1; forge, neoforge) | MIT | Unknown (withhold assets) | create | port (L) | Enchanting and experience automation | WS-4 | Modrinth exact file hash |
| Create Recycle Everything | `create_crush_everything` | `1.0.2` | [1.0.2](https://www.curseforge.com/minecraft/mc-mods/create-recycle-everything) (1.20.1; Forge) | ARR | ARR (until independently cleared) | None listed | port (S) | Create recycling recipes plus three new item types | WS-4 | CurseForge |
| Create: Ultimate Factory | `create_ultimate_factory` | `2.2.2` | [2.2.4](https://modrinth.com/mod/create-ultimate-factory) (26.2; fabric) | ARR | ARR (until independently cleared) | None listed | port (M) | Factory automation and Create recipes | WS-4 | Modrinth exact file hash |
| Create Crafts & Additions | `createaddition` | `forge-1.20.1-1.3.3` | [neoforge-1.21.1-1.7.2](https://modrinth.com/mod/createaddition) (1.21.1; neoforge) | MIT License, Copyright 2022 MRH0 | Unknown (withhold assets) | create | port (L) | Electricity, alternators and motors | WS-4 | Modrinth exact file hash |
| Cupboard | `cupboard` | `2.7` | [4.3](https://www.curseforge.com/minecraft/mc-mods/cupboard) (26.3; NeoForge) | ARR | ARR (until independently cleared) | None listed | library | Utility code shared by networking mods | dependent ports | CurseForge |
| Curios API | `curios` | `5.14.1+1.20.1` | [9.6.0+1.21.1](https://modrinth.com/mod/curios) (1.21.1, 1.21; neoforge) | LGPL-3.0-or-later | Unknown (withhold assets) | None listed | library | Equipment and accessory slots | dependent ports | Modrinth exact file hash |
| Easy Villagers | `easy_villagers` | `1.1.39` | [neoforge-1.1.49+26.3](https://modrinth.com/mod/easy-villagers) (26.3; neoforge) | ARR | ARR (until independently cleared) | None listed | port (M) | Villager conversion into functional utility blocks | WS-1 candidate | Modrinth project match |
| Embeddium | `embeddium` | `0.3.31+mc1.20.1` | [1.0.15+mc1.21.1](https://modrinth.com/mod/embeddium) (1.21.1; neoforge) | LGPL-3.0-only | Unknown (withhold assets) | None listed | client | Client rendering performance | client only | Modrinth exact file hash |
| Entity Culling | `entityculling` | `1.9.5` | [1.12.1](https://modrinth.com/mod/entityculling) (26.3; neoforge) | tr7zw Protective License | Unknown (withhold assets) | None listed | client | Client-side entity occlusion | client only | Modrinth exact file hash |
| Epic Knights | `magistuarmory` | `10.11` | [10.18](https://modrinth.com/mod/epic-knights-shields-armor-and-weapons) (26.1.2; neoforge) | ARR | ARR (until independently cleared) | architectury, cloth_config | port (L) | Medieval weapons, shields and armor | WS-1 future | Modrinth exact file hash |
| Explorer's Compass | `explorerscompass` | `1.20.1-1.3.3-forge` | [26.3-3.3.2-neoforge](https://modrinth.com/mod/explorers-compass) (26.3; neoforge) | CC BY-NC-SA 4.0 | Unknown (withhold assets) | None listed | port (M) | Locate structures via configurable compass | WS-1 candidate | Modrinth exact file hash |
| Farmer's Delight | `farmersdelight` | `1.20.1-1.2.9` | [1.21.1-1.3.4](https://modrinth.com/mod/farmers-delight) (1.21.1; neoforge) | MIT (source LICENSE) | MIT (source LICENSE) | None listed | port (L) | Food, crops, cooking and kitchen blocks | WS-1 (active port) | Modrinth exact file hash |
| FerriteCore | `ferritecore` | `6.0.1` | [9.0.0-fabric](https://modrinth.com/mod/ferrite-core) (26.1.2, 26.2, 26.3 (+2); fabric) | MIT | Unknown (withhold assets) | None listed | vanilla | Memory optimization | engine | Modrinth exact file hash |
| FLIB | `flib` | `0.0.15` | [26.1.2-0.2.11](https://modrinth.com/mod/flib) (26.1.2; neoforge) | MIT | Unknown (withhold assets) | None listed | library | Library for Simple Storage Network | dependent ports | Modrinth project match |
| Incendium | `incendium` | `5.3.5` | [5.3.5](https://modrinth.com/mod/incendium) (1.20.4, 1.20.5, 1.20.6 (+4); datapack) | Stardust Labs License | Unknown (withhold assets) | None listed | datapack | Nether world generation and structures | E2 | Modrinth exact file hash |
| Jade | `jade` | `11.13.2+forge` | [26.1.12+neoforge](https://modrinth.com/mod/jade) (26.1, 26.1.1, 26.1.2; neoforge) | CC BY-NC-SA 4.0 | Unknown (withhold assets) | None listed | client | Block and entity information overlay | WS-8 if needed | Modrinth exact file hash |
| Just Enough Items | `jei` | `15.20.0.129` | [31.14.0.72](https://modrinth.com/mod/jei) (26.3; neoforge) | The MIT License (MIT) | Unknown (withhold assets) | None listed | client | Item and recipe browser | WS-8 if needed | Modrinth exact file hash |
| L_Ender's Cataclysm | `cataclysm` | `3.16` | [3.33](https://modrinth.com/mod/l_enders-cataclysm) (1.21.1; neoforge) | Undetermined | Unknown (withhold assets) | lionfishapi | port (XL) | Structures, bosses and equipment | WS-5 | Modrinth exact file hash |
| Legendary Monsters | `legendary_monsters` | `2.0.6` | [2.2.4](https://modrinth.com/mod/legendary-monsters) (1.21.1; neoforge) | ARR | ARR (until independently cleared) | None listed | port (XL) | Hostile monsters and encounters | WS-6 | Modrinth exact file hash |
| Lionfish API | `lionfishapi` | `2.4-Fix` | [3.1](https://modrinth.com/mod/lionfish-api) (1.21.1; neoforge) | ARR | ARR (until independently cleared) | None listed | library | Support library for Cataclysm | WS-5 | Modrinth project match |
| Lithostitched | `lithostitched` | `1.4.11-forge-1.20` | [1.4.12-forge-20.1](https://modrinth.com/mod/lithostitched) (1.20.1, 1.20; forge) | MIT | Unknown (withhold assets) | None listed | library | Worldgen and datapack compatibility APIs | E2 / dependent ports | Modrinth exact file hash |
| Lootr | `lootr` | `0.7.35.94` | [1.24.41.124](https://modrinth.com/mod/lootr) (26.2; fabric) | [see license](https://github.com/noobanidus/Lootr/blob/1.16/LICENSE) | Unknown (withhold assets) | None listed | port (M) | Per-player loot containers | WS-1 candidate | Modrinth exact file hash |
| ModernFix | `modernfix` | `5.26.2+mc1.20.1` | [5.28.0+mc1.20.1](https://modrinth.com/mod/modernfix) (1.20.1; forge) | GNU LGPL 3.0 | Unknown (withhold assets) | None listed | vanilla | Runtime and memory optimizations | engine | Modrinth exact file hash |
| Mouse Tweaks | `mousetweaks` | `1.20.1-2.25.1-forge` | [26.3-2.31-forge](https://modrinth.com/mod/mouse-tweaks) (26.3; forge) | BSD-3-Clause | Unknown (withhold assets) | None listed | client | Inventory mouse shortcuts | client only | Modrinth exact file hash |
| Musket Mod | `musketmod` | `1.5.4` | [1.5.4](https://modrinth.com/mod/musket-mod) (1.21.1; neoforge) | MIT License | Unknown (withhold assets) | None listed | port (M) | Firearms, ammunition and projectile combat | WS-1 candidate | Modrinth exact file hash |
| Nature's Compass | `naturescompass` | `1.20.1-1.11.2-forge` | [26.3-3.3.1-neoforge](https://modrinth.com/mod/natures-compass) (26.3; neoforge) | CC BY-NC-SA 4.0 | Unknown (withhold assets) | None listed | port (M) | Locate biomes with a compass | WS-1 candidate | Modrinth exact file hash |
| Nullscape | `nullscape` | `1.2.8` | [2.0.1+26.3](https://modrinth.com/mod/nullscape) (26.3; datapack) | Stardust Labs License | Unknown (withhold assets) | None listed | datapack | End terrain and biome generation | E2 | Modrinth exact file hash |
| Patchouli | `patchouli` | `1.20.1-84.1-forge` | [1.20.1-85-forge](https://modrinth.com/mod/patchouli) (1.20.1; forge) | CC BY-NC-SA 3.0 | Unknown (withhold assets) | None listed | library | In-game documentation and book format | companion / dependent ports | Modrinth exact file hash |
| Polymorph | `polymorph` | `0.49.10+1.20.1` | [1.1.0+1.21.1](https://modrinth.com/mod/polymorph) (1.21, 1.21.1; neoforge) | LGPL-3.0-or-later | Unknown (withhold assets) | None listed | port (M) | Choose between conflicting crafting recipes | WS-1 candidate | Modrinth exact file hash |
| Quark | `quark` | `1.20.1-4.0-462` | [4.1-488](https://modrinth.com/mod/quark) (1.21.1; neoforge) | [see license](https://github.com/VazkiiMods/Quark/blob/master/LICENSE.md) | Unknown (withhold assets) | zeta | port (XL) | Large collection of mechanics and content | WS-1 future | Modrinth exact file hash |
| Recruits | `recruits` | `1.13.5` | [1.15.2](https://modrinth.com/mod/villager-recruits) (1.21.1; neoforge) | ARR | ARR (until independently cleared) | None listed | port (XL) | Recruitable NPC combat and AI | WS-1 future | Modrinth exact file hash |
| Refined Storage | `refinedstorage` | `1.12.4` | [2.0.9](https://modrinth.com/mod/refined-storage) (1.21.1; fabric) | MIT | Unknown (withhold assets) | None listed | port (XL) | Networked item and fluid storage | WS-1 future | Modrinth project match |
| Siege Weapons | `siegeweapons` | `0.2.3` | [0.2.5](https://modrinth.com/mod/siegeweapons) (1.20.1; forge) | ARR | ARR (until independently cleared) | None listed | port (L) | Siege equipment and combat physics | WS-1 future | Modrinth exact file hash |
| Simple Storage Network | `storagenetwork` | `1.12.2` | [26.1.2-1.13.2](https://modrinth.com/mod/simple-storage-network) (26.1.2; neoforge) | MIT | Unknown (withhold assets) | flib | port (M) | Networked chests and crafting terminals | WS-1 candidate | Modrinth project match |
| Small Ships | `smallships` | `2.0.0-b1.4` | [2.0.2-fabric-1.20.1](https://modrinth.com/mod/small-ships) (1.20.1; fabric) | ARR | ARR (until independently cleared) | None listed | port (L) | Pilotable ships and passengers | WS-1 future | Modrinth exact file hash |
| Sophisticated Backpacks | `sophisticatedbackpacks` | `3.24.20.1500` | [1.21.11-3.26.4.2261](https://modrinth.com/mod/sophisticated-backpacks) (1.21.11; neoforge) | ARR | ARR (until independently cleared) | sophisticatedcore | port (L) | Upgradeable backpacks | WS-1 future | Modrinth project match |
| Sophisticated Core | `sophisticatedcore` | `1.3.1.1460` | [26.3-1.5.40.2492](https://modrinth.com/mod/sophisticated-core) (26.3; neoforge) | ARR | ARR (until independently cleared) | None listed | library | Shared inventory mechanics for Sophisticated mods | dependent ports | Modrinth project match |
| Sophisticated Storage | `sophisticatedstorage` | `1.4.24.1486` | [1.20.1-1.5.19.2243](https://modrinth.com/mod/sophisticated-storage) (1.20.1; forge, neoforge) | ARR | ARR (until independently cleared) | sophisticatedcore | port (L) | Upgradeable storage blocks | WS-1 future | Modrinth project match |
| Steam 'n' Rails | `railways` | `1.6.15-beta+forge-mc1.20.1` | [1.7.3+forge-mc1.20.1](https://modrinth.com/mod/create-steam-n-rails) (1.20.1; forge, neoforge) | LGPL v3 | Unknown (withhold assets) | create | port (L) | Additional Create trains and tracks | WS-3 after P1d | Modrinth exact file hash |
| Tectonic | `tectonic` | `3.0.17` | [3.0.32-neoforge-26.3](https://modrinth.com/mod/tectonic) (26.3; neoforge) | MIT | Unknown (withhold assets) | lithostitched | datapack | Terrain shaping and world generation | E2 | Modrinth exact file hash |
| TerraBlender | `terrablender` | `3.0.1.10` | [3.3.0.12](https://modrinth.com/mod/terrablender) (1.20.4; neoforge) | LGPLv3 | Unknown (withhold assets) | None listed | library | Biome/worldgen integration APIs | E2 | Modrinth exact file hash |
| Terralith | `terralith` | `2.5.4` | [2.6.5+26.3](https://modrinth.com/mod/terralith) (26.3; datapack) | Stardust Labs License | Unknown (withhold assets) | None listed | datapack | Overworld biome and terrain data | E2 | Modrinth exact file hash |
| Waystones | `waystones` | `14.1.18` | [26.3.0.3+neoforge-26.3](https://modrinth.com/mod/waystones) (26.3; neoforge) | ARR | ARR (until independently cleared) | balm | port (M) | Fast-travel blocks and teleport menus | WS-1 candidate | Modrinth project match |
| Xaero's Minimap | `xaerominimap` | `forge-1.20.1-25.3.9` | [forge-26.3-26.6.2](https://modrinth.com/mod/xaeros-minimap) (26.3; forge) | ARR | ARR (until independently cleared) | xaerolib | client | Client minimap | client only | Modrinth exact file hash |
| Xaero's World Map | `xaeroworldmap` | `forge-1.20.1-1.40.10` | [forge-26.3-1.47.2](https://modrinth.com/mod/xaeros-world-map) (26.3; forge) | ARR | ARR (until independently cleared) | xaerolib | client | Client map display | client only | Modrinth exact file hash |
| Zeta | `zeta` | `1.20.1-1.0-31` | [1.1-40](https://modrinth.com/mod/zeta) (1.21.1; neoforge) | [see license](https://github.com/VazkiiMods/Zeta/blob/master/LICENSE.md) | Unknown (withhold assets) | None listed | library | Quark shared library | dependent ports | Modrinth exact file hash |

## Counts

port: 33, vanilla: 6, client: 8, library: 14, datapack: 4 (65 JARs).

## Source audit follow-ups

- Confirm the server install above is the intended buja pack. The directory is named `buja_server` and is referenced by `mods/create/README.md`, but the owner needs to confirm the pack snapshot and whether mods live in another launcher instance too.
- Check the actual license headers for every mod before copying code, data or art. For ARR or unclear licenses, port observable behavior from legally accessible source and use local original-jar assets; never commit the original textures/sounds/models.
- Verify the identities of the 11 name/mod-ID-matched Modrinth entries against their official authors and CurseForge listings when that mod is selected for porting. Modrinth project history may not include newer CurseForge-only releases.
- Some JAR manifests declare dependencies not present as loose JARs (for instance, Create references Flywheel/Ponder and some addons reference extra libraries). Check JarJar or packaged dependencies rather than assuming missing required mods.
- The packed 1.20.1 versions are inventory facts, **not** source code versions for the Rust ports. Each port uses the current upstream release, and its source, license and behavior must be pinned before implementation.

## Port ownership and staging

- Owned: Create, its add-ons and Steam 'n' Rails by Noel-Alex; Saints Dragons by sting-raider. Everything else is claimed through GitHub issues (`.agents/skills/work-loop`); the `Owner` column and the WS names below are historical. Cataclysm's work so far is in the private `devmogged/mods-private`.
- WS-1 candidates for separate M-sized substreams: Corpse, Waystones, Simple Storage Network, Explorer's Compass, Nature's Compass, Lootr, Polymorph, Easy Villagers and Musket Mod. They need feature-by-feature source study and may require ABI requests.
- Larger later WS-1 scopes: Farmer's Delight, Epic Knights, Small Ships, Sophisticated Backpacks and Storage, Recruits, Quark, Refined Storage, Alex's Caves and Alex's Mobs.
- The recipe-heavy Create Recycle addon is S-sized but belongs to WS-4. No independently safe S-sized WS-1 port is confirmed yet. Do not create a fake small port or port a library wholesale solely to keep this workstream busy.

## Local file manifest

Each line names the original JAR used for the `Bundled` column. These are metadata only; the JAR bytes are not checked in.

- `alexscaves`: `alexscaves-2.0.2.jar`
- `alexsmobs`: `alexsmobs-1.22.9.jar`
- `appleskin`: `appleskin-forge-mc1.20.1-2.5.1.jar`
- `architectury`: `architectury-9.2.14-forge.jar`
- `balm`: `balm-forge-1.20.1-7.3.28-all.jar`
- `canary`: `canary-mc1.20.1-0.3.3.jar`
- `carbonconfig`: `CarbonConfig-1.20-1.2.8.2.jar`
- `chunkpregen`: `Chunk-Pregenerator-1.20-4.4.5.1.jar`
- `chunksending`: `chunksending-1.20.1-2.8.jar`
- `chunky`: `Chunky-1.3.146.jar`
- `citadel`: `citadel-2.6.3-1.20.1.jar`
- `cloth_config`: `cloth-config-11.1.136-forge.jar`
- `connectivity`: `connectivity-1.20.1-7.6.jar`
- `corpse`: `corpse-forge-1.20.1-1.0.23.jar`
- `cmr`: `Create More Recipes-1.20.1-1.2.1-fix1.jar`
- `create`: `create-1.20.1-6.0.8.jar`
- `create_new_age`: `create-new-age-1.1.7f+forge-mc1.20.1.jar`
- `create_central_kitchen`: `create_central_kitchen-1.20.1-for-create-6.0.6-1.4.3b.jar`
- `create_copper_and_zinc`: `create_copper_and_zinc-2.0.0-forge-1.20.1.jar`
- `create_enchantment_industry`: `create_enchantment_industry-1.3.3-for-create-6.0.6.jar`
- `create_crush_everything`: `create_recycle_1.0.2_forge_1.20.1.jar`
- `create_ultimate_factory`: `create_ultimate_factory-2.2.2-forge-1.20.1.jar`
- `createaddition`: `createaddition-1.20.1-1.3.3.jar`
- `cupboard`: `cupboard-1.20.1-2.7.jar`
- `curios`: `curios-forge-5.14.1+1.20.1.jar`
- `easy_villagers`: `easy-villagers-forge-1.20.1-1.1.39.jar`
- `embeddium`: `embeddium-0.3.31+mc1.20.1.jar`
- `entityculling`: `entityculling-forge-1.9.5-mc1.20.1.jar`
- `magistuarmory`: `epic-knights-1.20.1-forge-10.11.jar`
- `explorerscompass`: `ExplorersCompass-1.20.1-1.3.3-forge.jar`
- `farmersdelight`: `FarmersDelight-1.20.1-1.2.9.jar`
- `ferritecore`: `ferritecore-6.0.1-forge.jar`
- `flib`: `flib-1.20.1-0.0.15.jar`
- `incendium`: `Incendium_1.20.x_v5.3.5.jar`
- `jade`: `Jade-1.20.1-Forge-11.13.2.jar`
- `jei`: `jei-1.20.1-forge-15.20.0.129.jar`
- `cataclysm`: `L_Enders_Cataclysm-3.16.jar`
- `legendary_monsters`: `legendarymonsters-2.0.6 MC 1.20.1.jar`
- `lionfishapi`: `lionfishapi-2.4-Fix.jar`
- `lithostitched`: `lithostitched-forge-1.20.1-1.4.11.jar`
- `lootr`: `lootr-forge-1.20-0.7.35.94.jar`
- `modernfix`: `modernfix-forge-5.26.2+mc1.20.1.jar`
- `mousetweaks`: `MouseTweaks-forge-mc1.20.1-2.25.1.jar`
- `musketmod`: `musketmod-1.20.1-forge-1.5.4.jar`
- `naturescompass`: `NaturesCompass-1.20.1-1.11.2-forge.jar`
- `nullscape`: `Nullscape_1.20.x_v1.2.8.jar`
- `patchouli`: `Patchouli-1.20.1-84.1-FORGE.jar`
- `polymorph`: `polymorph-forge-0.49.10+1.20.1.jar`
- `quark`: `Quark-4.0-462.jar`
- `recruits`: `recruits-1.20.1-1.13.5.jar`
- `refinedstorage`: `refinedstorage-1.12.4.jar`
- `siegeweapons`: `siegeweapons-1.20.1-0.2.3.jar`
- `storagenetwork`: `SimpleStorageNetwork-1.20.1-1.12.2.jar`
- `smallships`: `smallships-forge-1.20.1-2.0.0-b1.4.jar`
- `sophisticatedbackpacks`: `sophisticatedbackpacks-1.20.1-3.24.20.1500.jar`
- `sophisticatedcore`: `sophisticatedcore-1.20.1-1.3.1.1460.jar`
- `sophisticatedstorage`: `sophisticatedstorage-1.20.1-1.4.24.1486.jar`
- `railways`: `Steam_Rails-1.6.15-beta+forge-mc1.20.1.jar`
- `tectonic`: `tectonic-3.0.17-forge-1.20.1.jar`
- `terrablender`: `TerraBlender-forge-1.20.1-3.0.1.10.jar`
- `terralith`: `Terralith_1.20.x_v2.5.4.jar`
- `waystones`: `waystones-forge-1.20.1-14.1.18.jar`
- `xaerominimap`: `xaerominimap-forge-1.20.1-25.3.9.jar`
- `xaeroworldmap`: `xaeroworldmap-forge-1.20.1-1.40.10.jar`
- `zeta`: `Zeta-1.0-31.jar`

## WS-1 launch queue (M1)

Priority is based on players' visible gameplay, the size of a correct port, and the generic host interfaces it depends on. A high priority does not authorize a fake substitute while an ABI dependency is missing.

Before starting a port, read docs/PORTS.md, docs/MOD-ABI.md, the current main branch and the latest official source identified in the inventory, and claim it as described in AGENTS.md ("Working alongside other people"). First milestone for each is source and license verification, including the target release and its full Java call chain. Report generic host gaps as issues on `devmogged/moddedMC-rust`.

| Priority | Brief | Why | Current dependency |
| --- | --- | --- | --- |
| 1 | WS-1a Waystones (M) | Frequently used travel, distinct from the illustrative Wayshrine mod | Player/item activation, world saves, travel menus and cross-dimension teleport |
| 2 | WS-1b Corpse (M) | Death recovery affects every survival session | Death event, durable corpse inventory, retrieval |
| 3 | WS-1c Nature's Compass (M) | Important for biome hunting once E2 adds the worldgen packs | Bounded biome search, compass item state, destination UI |
| 4 | WS-1d Explorer's Compass (M) | Structure hunting is a core progression tool | Structure-location query and UI |
| 5 | WS-1e Polymorph (M) | Recipe conflicts are common in a large pack | Recipe enumeration, selectable result and craft authorization |
| 6 | WS-1f Easy Villagers (M) | Trading and farm automation | Vanilla villager interaction/trading and persisted workstations |
| 7 | WS-1g Lootr (M) | Per-player rewards matter with multiplayer | Loot containers, player-specific saved claiming state |
| 8 | WS-1h Simple Storage Network (M) | Quality-of-life storage across many containers | Container networking, terminals and menu clicks |
| 9 | WS-1i Musket Mod (M) | Player combat and equipment | Projectiles, reload timing, damage and packet events |

### WS-1a: Waystones
- Read first: [current release](https://modrinth.com/mod/waystones), latest official source, docs/PORTS.md and the existing mods/wayshrine example. Confirm version 26.3.0.3 or newer at start; do not confuse Wayshrines with the real Waystones port.
- Owns: mods/waystones, tests/scenarios/waystones-*.txt, docs/ws/waystones.md; suggested branch ws/buja-waystones.
- Milestones: M0 inspect the Java block/state, teleport menu, activation and saved location methods; M1 place/activate and persist every stone variant; M2 warp items, travel costs, ownership and menus; M3 original-mod scenarios, client verification checklist and perf. Assets require separate rights review (installed JAR says ARR).
- ABI: request only generic missing menu, item-use or persistent-storage operations from WS-0.

### WS-1b: Corpse
- Read first: [current release](https://modrinth.com/mod/corpse), death/revive/item-drop code and docs/MOD-ABI.md. Installed content is 1.0.23, latest tracked 26.3 NeoForge 1.1.22.
- Owns: mods/corpse, tests/scenarios/corpse-*.txt, docs/ws/corpse.md; branch ws/buja-corpse.
- Milestones: M0 read full death flow and request engine hooks; M1 spawn a corpse holding its actual inventory, no duplication; M2 save/reload and retrieve with all rules around despawn, dimensions and permissions; M3 original-mod death/recovery scenario and performance. ARR art cannot be committed.

### WS-1c: Nature's Compass
- Read first: [current release](https://modrinth.com/mod/natures-compass), biome enumeration, search order, radius, failure conditions and needle update code. Current official release covers Java 26.3.
- Owns: mods/naturescompass, tests/scenarios/naturescompass-*.txt; branch ws/buja-naturescompass.
- Milestones: M0 trace the biome search and file a world query request; M1 match search math and exact reference candidate ordering with unit cases; M2 item selection/target persistence, commands and bot scenario; M3 compare with original mod over seed-bound biome fixtures. Do not make a fake compass pointing to arbitrary coordinates.

### WS-1d: Explorer's Compass
- Read first: [current release](https://modrinth.com/mod/explorers-compass) and the source's structure filtering and search semantics. The official 26.3 release is tracked in the inventory.
- Owns: mods/explorerscompass, tests/scenarios/explorerscompass-*.txt; branch ws/buja-explorerscompass.
- Milestones: M0 inventory all selectable structures and target offsets; M1 generic locate-structure ABI request and exact search regression tests; M2 compass target and inventory state; M3 original-mod scenarios using fixed seeds. Depends on worldgen/structure parity in E2.

### WS-1e: Polymorph
- Read first: [current release](https://modrinth.com/mod/polymorph), the source's recipe-conflict resolution, visible choices and persistent selection model.
- Owns: mods/polymorph, tests/scenarios/polymorph-*.txt; branch ws/buja-polymorph.
- Milestones: M0 trace crafting match/select/consume order; M1 enumerate conflicting recipes and design a generic recipe-choice interface with WS-0; M2 crafting table/furnace integration without dupes; M3 original-mod reference fixtures and owner UI check. Respect the LGPL license and separate asset rights.

### WS-1f: Easy Villagers
- Read first: [current release](https://modrinth.com/mod/easy-villagers) and NeoForge villager block/entity classes. The latest tracked release targets Java 26.3.
- Owns: mods/easy_villagers, tests/scenarios/easy_villagers-*.txt; branch ws/buja-easy-villagers.
- Milestones: M0 trace capture, villager NBT, trade and workstation callbacks; M1 correct vanilla-villager storage and release; M2 automated trading/farming/breeding per original; M3 cold-restart and original-mod parity. Coordinate E6 vanilla trading; do not implement engine AI or menus locally.

### WS-1g: Lootr
- Read first: [current release](https://modrinth.com/mod/lootr), chest/open triggers and per-player loot algorithms; inspect code and art licenses.
- Owns: mods/lootr, tests/scenarios/lootr-*.txt; branch ws/buja-lootr.
- Milestones: M0 map original loot-table calls and opener identifiers; M1 per-player loot allocation under the generic loot/container interfaces; M2 multiplayer claims and restart persistence without dupes; M3 same-seed original-mod oracle and load test.

### WS-1h: Simple Storage Network
- Read first: [current release](https://modrinth.com/mod/simple-storage-network), FLIB compatibility and the original graph, terminal and inventory synchronization code.
- Owns: mods/storagenetwork, tests/scenarios/storagenetwork-*.txt; branch ws/buja-storage-network.
- Milestones: M0 map cable adjacency and menu requirements; M1 graph and item routing with fixture tests; M2 terminal and remote access using WS-0 menu services; M3 chunk unload/restart tests and reference stress benchmark. Keep networking mechanics inside this mod.

### WS-1i: Musket Mod
- Read first: [official source](https://github.com/ewewukek/mc-musketmod) and [latest original 1.5.4](https://modrinth.com/mod/musket-mod/version/1.5.4) for NeoForge 1.21.1. Do not accidentally port the separately maintained Reloaded fork.
- Owns: mods/musketmod, tests/scenarios/musketmod-*.txt; branch ws/buja-musket.
- Milestones: M0 inspect shot, reload, ammo, projectile and damage code; M1 item definitions and correct recipes; M2 implement projectile/weapon behavior after generic WS-0 operations arrive; M3 test shoot/reload against Java with fixed targets, item state across restart and owner visuals. MIT code, assets require a separate check.

### Current WS-1 implementation

This WS-1 chat owns the first Farmer's Delight crate and ingredient slices under mods/farmersdelight. More L-size content follows only through small independently testable stages and generic engine interfaces from WS-0. Do not open another workstream that edits the same crate until it is handed off. The full game is not covered by the first slices.
