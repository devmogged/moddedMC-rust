# WS-6: Legendary Monsters

## Scope and provenance

Workstream 6 on `ws/legendary-monsters`, owning only `mods/legendary-monsters`, `docs/ws/legendary-monsters*.md`, and WS-6 coordination requests. Server target: Java 26.3, original mod: Legendary Monsters **2.2.4** for NeoForge **Minecraft 1.21.1** (released 2026-10-23; CurseForge file 9301784). Different vanilla versions require explicit cross-version oracle comparisons, not guesswork.

Original release: https://www.curseforge.com/minecraft/mc-mods/legendary-monsters/files/9301784
Original license: https://www.curseforge.com/minecraft/mc-mods/legendary-monsters/license
Exact archived release SHA-256: `5A659453D7F439F36A1CDF0CB74EB29CFA98D2B6C693424157E2B67ECB16E136`. JAR is ignored in `cache/`.

**License constraint:** the publisher expressly permits re-use of source code but reserves names and assets. Do not commit original models, textures, sounds, language files, art, or copies of binary structure templates. Resolve the status of data files separately before publishing recipe/loot/worldgen payloads. Server reads authorized assets from an installed local original JAR, using the existing content fallback.

Public source (`Miauczel/Legendary-Monsters-1.21.1-NeoForge`) at `f412a3e` is dated 2026-07-15, so it predates the current October release. It is only a navigation aid; authoritative classes were decompiled from the exact 2.2.4 JAR to `D:/mc-ref/legendary-monsters-2.2.4-decompiled/` outside git.

## M0: Survey

See [the complete identifier and datapack-path inventory](legendary-monsters-inventory.md), generated directly from the 2.2.4 JAR and its decompiled registry methods.

- **46 blocks**, each automatically receiving a block item, plus **132 explicit items** (**178 item IDs** total). Raw resources include 76 loot-table paths and 158 recipe entries, of which 157 are `.json` recipes; the exceptional `recipe/atmospheric_boots` lacks an extension.
- **92 registered entity types**: **32 MONSTER**, **2 CREATURE**, **58 MISC**. The latter are not normal mobs: projectiles, impact zones, displays and control entities. Do not register all 92 as mobs.
- **12 effects**, **8 custom damage types**, **2 custom structure codec IDs** (`lm_jigsaw`, `cp_kingdom`), **13 worldgen structure JSON definitions** and **72 NBT structure templates** (plus unrelated `.idea` XML accidentally packaged in the structure directory), as well as configured and placed ore features, template pools, tags and NeoForge biome modifiers.
- Boss candidates confirmed by the original class hierarchy: **Cloud Golem**, **Possessed Paladin** and **The Obliterator** extend `IAnimatedBoss`. Eleven other entity classes in the older source extend `IAnimatedMiniBoss`; the latest 2.2.4 class hierarchy and precise spawn/phase rules must be checked individually in M2/M3. `TheObliteratorEntity` alone has thousands of Java lines and many custom goals and projectiles.
- No mod-specific enchantment registry or `data/legendary_monsters/enchantment/` entries found in the 2.2.4 inventory. This is a negative registry check, not proof that no item interacts with vanilla enchantments.

### Read and cross-checked

`ModBlocks.registerBlock`, `ModItems.ITEMS.register`, `ModEntities.ENTITY_TYPES.register`, `ModEffects.MOB_EFFECTS.register`, `ModStructures.STRUCTURE_TYPE_DEF_REG.register` were checked in **2.2.4 decompiled source**. Item/block/recipe lists are exact IDs, not descriptions of behavior. Also read the existing Saint's Dragons mob port and `moddedmc:mobs`/`moddedmc:mob_ai` API.

Current host has generic item/content definitions, a saved mob-state carrier, AI goals, entity movement/lookup and basic effects. Behavior needed by this mod but not yet exposed: damage/death interception and damage-source attribution; boss bars; modded projectiles with owner, collision and server tick; mod status effects/custom damage types; natural spawn rules and structures; item use in air and unusual weapon abilities. The existing WS-5-01 through WS-5-08 requests already cover shared combat, active item, projectile, boss-bar, natural spawn, custom structure, effect and menu gaps; do not duplicate them. WS-6-01 requests the additional generic explosion operation, grounded in 2.2.4 `BombEntity::onHit`. ABI requests are tracked outside git under `N:/mmc-coord/requests/` and are pending WS-0.

### Verification

M0 is documentation only, so it has no in-game behavior to exercise. Verified JAR download integrity by SHA-256, decompiled registry source, counted unique registration keys with an automated script, parsed 157 valid recipe JSON objects into five vanilla recipe types, and classified all entity registrations by their explicit Minecraft mob categories. **Not run:** original NeoForge mod on a Java server, bot gameplay, vanilla differential harness, or Rust tests. This does **not** establish gameplay parity. Shared engine binaries were not published at M0 setup. Independent WS-R review still required before calling the milestone accepted.

## Next: M1 items/blocks

Port the easiest passive materials and structural blocks from the 2.2.4 methods first, with their actual templates and state properties. Then add active items only after tracing their complete `use`, `useOn`, `hurtEnemy`, projectile/effect and stack/NBT methods. Use a test scenario per player-visible slice, with restart for stateful mechanics; compare outcomes against NeoForge 1.21.1 v2.2.4 before claiming parity. Never fake abilities as basic vanilla items. No inherited private assets in the public tree.

## Later order

M2: **ordinary mobs** with spawn criteria, AI, attacks, drops and persistent state, one per verified step. M3: bosses, phases, linked projectiles and boss bars, one at a time. M4: structures, template pool selection, placement and cross-version worldgen checks once E2 is available. Use WS-6 ports **26060–26069**, shared binaries only, no engine build.
