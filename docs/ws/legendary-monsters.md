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

## M1 progress: 24 passive materials (checkpoint, not full M1)

Created `mods/legendary-monsters/` as an independent native Rust cdylib crate, with `.mmc` id `legendary-monsters` and original content namespace `legendary_monsters:`. It registers 24 `ModItems.java` entries whose source constructors contain only default vanilla properties or `fireResistant()` (five items). Two original crafting conversions are implemented in native 26.3 recipe JSON: one `enderitium_ingot` becomes 9 `enderitium_gem`, and a 3×3 grid of gems becomes one ingot. The original third ingot recipe, using `enderitium_block`, is intentionally withheld until the block is implemented with its correct physics. Vanilla item carrier types handle max stack 64 and fire resistance without reimplementing item handling. Exactly 24 corresponding model JSON paths were confirmed in the original 2.2.4 JAR; no original texture, model, loot, language or NBT assets are checked in; only two small source-derived crafting definitions are authored in the port. The packaged crate contains a license/provenance notice.

Verification completed: source registration/property audit for all 24 selected IDs; `cargo fmt --package legendary-monsters --check`, `cargo clippy -p legendary-monsters --all-targets -- -D warnings`, `cargo test -p legendary-monsters` (0 unit tests, since no algorithmic stateful logic was added), and `scripts/pack-mod.ps1 -Crate legendary-monsters`. The Windows .mmc archive contains `mod.json`, the native DLL, the source-license notice and the two original crafting conversions. The bot scenario also **PASSED** loading exactly 2 recipes from the mod datapack and unlocking both via `/recipe give`. Scenario `tests/scenarios/legendary-monsters-materials.txt` **PASSED** against the WS-0 shared server/bot (`engine=d6e5e8d560`, ABI 1) on port **26060**: registered 24 material IDs, the bot received 2 Dinosaur Bones, 4 Chorus Crystals and 1 Metal Debris, and `/clear <player> <item> 0` reported those exact persistent counts after a full stop/restart/rejoin. All checks passed, no panic. A first run had six *test-expectation* misses because host registration logs are sorted alphabetically instead of registration-source order; the scenario was corrected to check a single registration and exercise distinct IDs through real commands. The server found original assets in the ignored 2.2.4 JAR, but warned that some content pack assets are missing and uses a placeholder; visual parity remains unverified. No original NeoForge 2.2.4 oracle or Java differential fixture has been run. **Do not call the full M1 milestone accepted.**

Remaining M1: the other explicit item IDs, item models in a legally supplied local asset source, original datapack recipes and loot usage, block registration, tools, equipment, food, action callbacks and their oracle-based scenarios. Property parity is limited by the vanilla carrier's other metadata and must be tested; custom rarity and foods are deliberately omitted. **Block physics requires WS-6-02:** original `ModBlocks.java` overrides copied vanilla hardness and explosion resistance, but `content::State.physical` only copies the stand-in's full properties. Even an apparently trivial `enderitium_block` would have the wrong explosion resistance if registered as a vanilla netherite block. Do not add unfaithful blocks until WS-0 supplies a generic per-state physical override.

## M1 continuation: recipes, tags, crafting transactions and block audit

- Added **2 original crafting recipes** as authored JSON in `mods/legendary-monsters/resources/data/legendary_monsters/recipe`: `enderitium_gem` (1 Enderitium Ingot -> 9 Enderitium Gems, shapeless) and `gem_enderitium_ingot` (3x3 Enderitium Gems -> 1 Enderitium Ingot, shaped). Both have equal parsed JSON semantics to the corresponding exact 2.2.4 JAR recipes.
- Added **2 additive item-tag entries** (`minecraft:smithing_table_materials`, `minecraft:trim_materials`) for registered `enderitium_ingot` from original tags. Omitted `portal_shard` because that item is not ported. Bot confirms the datapack loads 2 recipes and 2 tags, and that both recipe IDs unlock. Tag *membership use* has not been exercised; an attempted `execute if items` assertion failed because the current host does not implement that command syntax and was replaced with a supported item-slot check.
- `tests/scenarios/legendary-monsters-crafting.txt`: **PASS** with a bot on port **26062** (11 seconds). Bot places an ingot into the 2x2 inventory crafting grid and shift-clicks output; `/clear ... 0` confirms **9 gems** actually crafted, not merely unlocked.
- `tests/scenarios/legendary-monsters-crafting-table.txt`: **PASS** on port **26064** (51 seconds). Bot places nine gems into the 3x3 crafting-table grid, shift-clicks result; command check reports **1 ingot and zero gems**. After full stop/restart/rejoin, the ingot is still present and the gems absent.
- The existing `legendary-monsters-materials.txt` scenario **PASS** on port **26060** (rechecked after new recipes/tags), including registration, both recipe unlocks, material `/give`, an ingot item slot, and exact saved quantities after restart.
- Added `tools/audit_artifact.py`: hashes the local 2.2.4 original JAR, checks all 24 declared item IDs, original model paths, the five fire-resistant flags, exact recipe JSON and tag subsets. **PASS** with pinned SHA-256 and exact decompiled `ModItems.java`. All checks use installed original artifacts; no restricted original assets are published.
- Completed `legendary-monsters-blocks.md` from `ModBlocks.java` and the exact 2.2.4 JAR: all **46** block registrations, source vanilla property bases/strength/sounds/tool tags, **39** named loot tables, **37** block models and **46** item models; **9** special block behavior classes. None is misrepresented as fully ported; WS-6-02 is needed for correct block physics.

**Review status:** WS-R marked M0 commit `775e596` **OK**. WS-R marked M1 checkpoint `a7a18b5` **fix needed** solely because the early scenario chained out-of-order registration log expectations; this scenario is corrected and independently re-ran with passing gameplay, but reviewer re-acceptance remains pending.

**Remaining parity:** all modded blocks and remaining items, detailed tool properties, mob AI/combat, boss mechanics and structure placement await later milestones and generic host interfaces. Neither original-mod NeoForge 1.21.1 game oracle nor a real-client visual test has been performed. No claim of full M1 parity or completeness.

## Later order

M2: **ordinary mobs** with spawn criteria, AI, attacks, drops and persistent state, one per verified step. M3: bosses, phases, linked projectiles and boss bars, one at a time. M4: structures, template pool selection, placement and cross-version worldgen checks once E2 is available. Use WS-6 ports **26060–26069**, shared binaries only, no engine build.
