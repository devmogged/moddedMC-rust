# WS-1: buja inventory and small ports

## M0: inventory (provisional, 2026-11-20)

Branch: `ws/buja`. Worktree: `N:\mmc-wt\buja`. Scope is inventory and small independent mods; Create, addons, engine and the ABI belong to other workstreams.

The owner confirmed that **the entire buja pack** is in scope and every port must use the latest official mod release. The user-supplied path `D:\buja_server` is absent on this laptop. The only installed Forge buja server located is `D:\minecraft\buja_server\mods`, also referenced in `mods/create/README.md`. Its 65 JAR basenames match exactly the 65 JAR basenames in the neighboring `mods.zip` archive (67 ZIP entries total); there are no archive-only or loose-only JARs. The extra `temp.py` is not a mod. This 65-JAR working snapshot is inventoried in [`docs/BUJA.md`](../BUJA.md), pending authoritative path mapping and per-mod rights review. No original jars/assets are committed.

Evidence: read embedded `META-INF/mods.toml` / `fabric.mod.json`, extracted declared mod IDs, versions, dependencies and license fields, and matched installed SHA-1 against Modrinth. There are 48 exact file-hash matches, 11 additional project-title/mod-ID matches requiring publisher checks, and 6 official CurseForge release-page checks. Each row records its source and the latest published stable release found on that service, with its game version and loader. This is not a claim that all projects have been checked against every distribution site.

Provisional categories: 33 server ports (including Create), 6 engine/vanilla replacements, 8 client-only mods, 14 libraries, 4 data/worldgen mods. Code-license labels are the installed JARs' claims; independent license-file and asset redistribution checks are still needed for every chosen port. Create Recycle Everything has 6 compiled classes, including three custom item classes, and 228 `data/` entries, so it is a small WS-4 addon port, not merely a data pack. Create More Recipes provides custom fan and spouting recipe types, so its runtime must not be skipped.

Verification performed: every JAR was readable as ZIP; exactly 65 unique candidate filenames appear in the source metadata export and inventory. Inventory has 65 mod rows and 65 corresponding manifest entries. No native gameplay scenarios were run: this milestone changes documentation only, and WS-0 has not published shared engine binaries. Real mod parity and asset licenses remain unverified. No Git changes were made in the integrator's checkout.

**M0 review remains open:** the owner confirmed scope, but the literal supplied path `D:\buja_server` is missing and the nested source is the only identified installation. Verify whether an alternate newer launcher snapshot exists before declaring the 65 JARs authoritative. Rights status is explicitly `Unknown (withhold assets)` for unresolved items; code-license metadata claims also need release-level verification before porting. The Farmer's Delight 1.3.4 source was separately verified MIT. See the reviewer findings in `N:\mmc-coord\reviews\WS-1\b8dfef6.md`.

## M1: ranked port briefs (prepared; owner launches separate chats)

Prioritize by visible player impact, ability to implement actual gameplay through the current ABI, and size. Candidate first substreams for owner-launched separate chats: `WS-1a` Waystones (high visibility, needs teleport/ownership and menu parity), `WS-1b` Corpse (death persistence, item recovery; may need entity and inventory hooks), `WS-1c` Nature's Compass (biome search/world queries), `WS-1d` Explorer's Compass (structure search), `WS-1e` Polymorph (recipe selection UI), `WS-1f` Easy Villagers (villager interactions and workstations), `WS-1g` Lootr (per-player container loot), `WS-1h` Simple Storage Network (inventory network and menus), and `WS-1i` Musket Mod (projectiles and weapon events). These are provisional; versions, licensing, source methods and ABI requirements must be rechecked when each starts. Do not spawn any of these parallel workstreams from WS-1; the owner starts them.

L/XL follow later: Farmer's Delight, Epic Knights, Small Ships, Sophisticated storage/backpacks, Quark, Refined Storage, Recruits, Alex's Mobs and Alex's Caves. Create and its addons, dragons, Cataclysm and Legendary Monsters are owned by their other designated workstreams. No standalone S-sized WS-1 candidate has passed its source/ABI survey; do not manufacture a placeholder mod.

## Next

Continue small source-verified Farmer's Delight slices in this worktree, using ports 26010-26019 and direct original-code comparisons. Keep the two generic WS-0 ABI requests tracked without implementing host code in WS-1. Close the M0 pack-path and per-mod rights questions against the owner-confirmed pack scope, and leave the nine independently owned medium ports for the owner's separate chat/worktree launches. Require WS-R review before any integrator merge.

## M2: Farmer's Delight crop crates (first playable slice)

Branch commit: recorded on push. Source: official vectorwing/FarmersDelight 1.21.1-1.3.4, revision dd8b9b0 (2026-08-29), from D:/mc-ref/farmers-delight-1.21.1-1.3.4. Original files: common/registry/ModBlocks.java (CARROT_CRATE, POTATO_CRATE, BEETROOT_CRATE: ordinary Block with oak-plank physics, strength 2.0/3.0, wood sound); common/registry/ModItems.java (three ordinary BlockItems); generated recipes, loot tables, models, and textures. LICENSE (MIT) included in the .mmc. No other Farmer's Delight mechanics are claimed.

Added mods/farmersdelight as an independently built native crate with three blocks, six original packing/unpacking recipes, loot tables, models, textures and the original axe mineability tag narrowed to these blocks. Removed only NeoForge's condition for the default-true enableVanillaCropCrates option (see Configuration.java); the client-side configuration toggle is not ported.

Checks run in WS-1 worktree:
- cargo fmt --all --check: PASS.
- cargo clippy -p farmersdelight --all-targets -- -D warnings: PASS.
- cargo test -p farmersdelight: PASS (zero unit tests; this slice has no branchy logic).
- powershell -NoProfile -File scripts/pack-mod.ps1 -Crate farmersdelight: PASS (69,095-byte farmersdelight.mmc).
- powershell -NoProfile -File scripts/scenario.ps1 -Scenario tests/scenarios/farmersdelight-crates.txt -RunDir run/buja-fd-crates -Fresh -Port 26010 -Exe ..\..\mmc-shared\bin\d6e5e8d560\pack-dev.exe -BotExe ..\..\mmc-shared\bin\d6e5e8d560\moddedmc-bot.exe: PASS. The bot fetched a resource pack, placed all three crates; commands confirmed states, correct axe tag and absence from pickaxe tag, unlocked six recipes, generated the correct named loot drops, and found all three blocks retained after full server restart. Run dir was cleaned.

Unverified: the actual 3x3 crafting GUI output and stack consumption, visual model in a real stock client, the Java 1.21.1 NeoForge original-mod oracle, and the disabled-crates config option. The simulation's native recipe registrations plus copied JSON are narrower evidence than full original-mod parity. Before claiming this portion complete per section 7, perform the original Java comparison and owner visual play test.

Next content slice: basic ingredients straw/canvas/tree bark/rice panicle, then more of the small genuinely passive content with source-checked model/recipe data. Gameplay features requiring tool, crop, food, or cooking APIs must be requested through WS-0 and tested separately.

## M1: launch briefs (ready for owner allocation)

The ranked queue and individual scopes are in docs/BUJA.md under "WS-1 launch queue". Nine medium-size candidates have explicit source links, ownership boundaries, prerequisites and staged milestones. The owner starts separate chats after allocating non-overlapping worktrees and ports. M0 inventory remains provisional until the intended pack snapshot is confirmed.

## M2b: Farmer's Delight basic materials (verified native slice)

Source remains official Farmer's Delight 1.3.4, revision dd8b9b0. New item registrations exactly follow ModItems.java: STRAW, CANVAS, TREE_BARK, RICE_PANICLE use new Item(basicItem()) with no custom class or extra behavior. Registered as 64-stack vanilla-like items through moddedmc:items v1, with original item models/textures. Copied the original vanilla-format canvas, book_from_canvas, painting_from_canvas and scaffolding_from_canvas recipes. No new host interface was needed.

Tests:
- cargo fmt --all --check: PASS.
- cargo clippy -p farmersdelight --all-targets -- -D warnings: PASS.
- cargo test -p farmersdelight: PASS, 0 Rust unit tests because this slice has only literal definitions.
- powershell -NoProfile -File scripts/pack-mod.ps1 -Crate farmersdelight: PASS, 74,262-byte package.
- tests/scenarios/farmersdelight-ingredients.txt using WS-0 shared pack and bot, port 26011: PASS. Four items given with exact names, four recipe IDs unlocked, fetched resource pack, 3 straw persisted in player inventory across logout and restart (clear ... 0 found 3).
- tests/scenarios/farmersdelight-crates.txt on port 26010, with the expanded 10-recipe pack: PASS. All three crate placements, loot and saved-world reload checks passed again.

Unverified: obtaining straw through cutting with a knife, harvesting rice panicles, tree bark extraction and original-Java recipe output and rendering comparison; none is claimed implemented. Source fixtures give implementation evidence for the passive items only. Continue to expand in stages after checking the original class behavior, implementing only exposed mechanics.

## M2c: real crafting regression (verified)

Added `tests/scenarios/farmersdelight-crafting.txt`: an actual Survival-mode bot opens a vanilla crafting table, takes nine vanilla carrots/potatoes/beetroots from its hotbar, places them one at a time into the 3x3 grid using inventory clicks, quick-moves the crafted `farmersdelight:<crop>_crate`, then confirms exactly one crate and zero remaining source crops. For each crop, it removes that crafted crate, tests the reverse shapeless recipe with a known crate stack, confirms exactly nine restored crops and no leftover crate, then clears the crops before the next case. The bot checks cursor empty after distribution. All three round trips passed on shared pack `d6e5e8d560`, port 26012; run folder automatically removed. No crashes or bot protocol errors.

This is the first gameplay test for actual recipe execution rather than only recipe registration. It exercises the old exact 1.21.1 Java recipe JSONs in the Java 26.3 server. Java 1.21.1 NeoForge oracle and owner graphics checks remain unverified.

## M2d: real canvas recipe regression (verified)

Added `tests/scenarios/farmersdelight-canvas-crafting.txt` on WS-1 port 26013. A Survival bot opened the crafting table and placed the original items into the exact recipe pattern for four original 1.21.1 Farmer's Delight recipes: 4 straw to 1 canvas, 3 paper plus canvas to 1 vanilla book, 8 sticks plus canvas to 1 vanilla painting, and 6 bamboo plus canvas to 6 vanilla scaffolding. For each test the server confirmed the exact output count, no leftover input items, and proper result removal; every check passed, no crash or bot error, and the run directory was deleted. Recipe registration alone is no longer the only evidence for these outputs.

## M2e: more original storage blocks (verified native slice)

Ported four further plain Block/BlockItem registrations from official Farmer's Delight 1.3.4 source: cabbage crate, tomato crate and onion crate exactly follow oak-plank physical properties, while rice bag follows white-wool physical properties. Copied the original four blockstate, block model, item model and loot files with their textures. The three new crop crates are added to the axe mineability tag exactly as in original data; rice bag is not in that axe tag. No recipes for these four blocks were imported: the upstream recipes require cabbage, tomato, onion or rice items whose gameplay semantics are not yet implemented. They can be Creative-given, placed, broken and saved, but cannot yet be Survival-crafted.

Verification: cargo fmt --all --check, cargo clippy -p farmersdelight --all-targets -- -D warnings and cargo test -p farmersdelight (0 unit tests) PASS; native package built to 86,002 bytes. `run/verify_fd_reference.py` PASS for seven original Java block IDs, four basic item definitions, 10 recipe JSON semantic matches, seven loot tables, all referenced models/textures, axe tag and MIT license. `tests/scenarios/farmersdelight-storage-extra.txt` on WS-1 port 26014 PASS: four real bot placements, correct loot, the source axe tag and all four saved blocks reloaded after a cold restart. All four existing Farmer's Delight scenarios (crates, ingredients, three-crate real crafting, four canvas crafts) re-ran and PASS with the updated seven-loot-table data pack and were cleaned automatically. The original Java oracle and real-client visuals remain unverified.

## M2f: remaining source-compatible material recipes (verified)

Surveyed official 1.3.4 source `src/generated/resources/data/farmersdelight/recipe/` for vanilla shaped, shapeless, cooking and stonecutting recipes whose ingredients and outputs exist in the current native port or vanilla. Thirteen source recipes qualified (all 10 already ported plus the three below). Copied three source-identical recipe JSON files without changing their conditions: `lead_from_straw` (five straw to one lead), `packed_mud_from_straw` (one straw and two mud to two packed mud), `paper_from_tree_bark` (three bark to one paper). No new block or item registration.

Verification: `tests/scenarios/farmersdelight-material-crafting.txt` on port 26015 PASS, exercising the actual Survival crafting table with inventory clicks and confirming exact output quantities and consumption of every input; no recipe-only unlock shortcuts. Source audit `run/verify_fd_reference.py` PASS for all 13 recipe objects (the only normalization remains removal of the default-true NeoForge vanilla-crates condition), seven blocks, four materials, seven loot tables, original models/textures and license. `cargo fmt --all --check`, strict `cargo clippy -p farmersdelight --all-targets -- -D warnings` and `cargo test -p farmersdelight` PASS. Native package now 86,976 bytes. All five previously existing Farmer's Delight scenarios (crates, ingredients, original crate crafting, canvas crafting, new storage blocks) reran and PASS with 13 source recipes and seven loot tables, in addition to the new material-crafting scenario. All six successful run directories were cleaned.

Further Farmer's Delight recipes depend on food/crop registrations, complex processing, or other unimplemented mechanics; do not load broken or semantically false recipes. ABI requests WS-1-01 (food components) and WS-1-02 (thin block physical and survival behavior) remain pending. Original NeoForge Java oracle and stock-client play test are still required for formal parity.

## M2g: 72-cutting-recipe source evaluator (pure Rust, tests pass; gameplay integration pending)

Official Java source pinned: vectorwing/FarmersDelight 1.21.1-1.3.4, revision `dd8b9b0` (MIT), `common/crafting/CuttingBoardRecipe.java`, `common/crafting/ingredient/ChanceResult.java`, and `src/generated/resources/data/farmersdelight/recipe/cutting/*.json`. Copied and compared all 72 source recipe JSONs verbatim into `mods/farmersdelight/tests/fixtures/cutting/` (32,420 bytes). They are test fixtures, **not** active datapack recipes.

`mods/farmersdelight/src/cutting.rs` implements the source's one-item `Ingredient` and tool OR-alternative matching (tag, item ability), `MAX_RESULTS=4` validation, default chance 1.0, optional result components and sound ID, and `ChanceResult.rollOutput` one RNG float per output unit including guaranteed results. The threshold uses Java's float-to-double promotion for `chance` and the configured per-level Fortune bonus (0.1 by default), comparing `nextFloat() > threshold` as original (implemented by retaining rolls at `<=`).

Verification: `cargo fmt --package farmersdelight --check` PASS; `cargo test -p farmersdelight --locked` **5 pass / 0 fail** including parsing all 72 original recipes, matching tags and item abilities, Fortune/chance boundaries, draw accounting and rejection of malformed recipe types; strict `cargo clippy -p farmersdelight --all-targets --locked -- -D warnings` PASS. `scripts/pack-mod.ps1 -Crate farmersdelight` PASS (86,975-byte native `.mmc`). Existing live Survival-bot `farmersdelight-material-crafting.txt` on WS-1 port 26015 PASS after the package rebuilt: 13 vanilla-compatible recipes and seven loot tables load, actual table crafting consumes inputs, no crashes. The cutting evaluator is **not yet attached to an interactable cutting-board block** and must not be called a finished gameplay milestone. Do not add the 72 cutting recipes to vanilla recipe/datapack paths until the mod-specific cutting recipe type is handled.

Blocked next: generic host operations in coordination `N:\mmc-coord\requests\WS-1-03.md` (tool durability, custom block container exposure, item ability and comparator); thin block physics/support in `WS-1-02.md`. When the host supports these, register the original cutting board and knife item(s), load source recipes via `moddedmc:resources`, test placement, block-entity NBT/restart, tool wear, original output drops, Fortune and exact comparison with a Java NeoForge oracle. Maintain this independently staged module until those tests pass.

## M2h: FoodValues reference extraction (source verified; item gameplay pending WS-0)

Pinned MIT upstream is still Farmer's Delight 1.3.4, revision `dd8b9b0`. Exact `common/FoodValues.java` was copied into `mods/farmersdelight/tests/fixtures/FoodValues.java` and SHA256-verified against the upstream checkout (48139628B2BBD5FEC04D3082F75192B40D22011035D21A076A5D00F1772DF48C). Added `tools/extract_food_values.py` to extract every directly declared food and vanilla soup override, fail closed on unknown builder calls, and compare a canonical `food_values.json` to the pinned source on `--check`.

Exactly 74 FoodProperties constants were recovered (73 known builder expressions and one direct-constructor special case `KELP_ROLL` retained as literal Java, explicitly marked for manual semantic review), plus three vanilla soup override rules. The structured reference preserves nutrition, saturation modifier and its original Java literal, fast/always-edible flags, returned bowl, 37 source effect declarations, effect chance as float literal, duration, amplifier and explicitly specified particle/ambient flags. Absent/defaulted fields are `null` and **not guessed**. The default Nourishment durations 600/1200/3600/6000 ticks are resolved from source constants.

Rust `src/food_values.rs` provides once-only typed manifest parsing and lookup for future registration, with four tests asserting all 74/3 entries and the original raw crop/effect/special-constructor values. Verification: `py -3 mods/farmersdelight/tools/extract_food_values.py --check` PASS; `cargo test -p farmersdelight --locked` **9 PASS** (previous five cutting tests plus four food tests); `cargo fmt --package farmersdelight` and strict `cargo clippy -p farmersdelight --all-targets --locked -- -D warnings` PASS. This is source reference data, not Minecraft 26.3 food components and does not register edible items. WS-1-01 (generic item default-component interface) remains a blocking host request; no substitute vanilla carrier was introduced.

## M0 byte-level buja archive verification (2026-11-20)

Implemented reproducible `scripts/verify-buja-snapshot.py` with streaming SHA-256 of all JARs from both `D:\minecraft\buja_server\mods\` and `D:\minecraft\buja_server\mods.zip`. For this owner-approved 65-mod scope, **65/65** JARs have identical filenames, uncompressed lengths and SHA-256 byte digests, with no missing entries. The canonical order-independent name+digest fingerprint is `05d095e348adbc85e4e1c706f1ed1d0e8197bd99e3c9b79d477f4d6f9424d888`, and the ZIP SHA-256 is `40aa170fa1f863d90cdc507762c366d3933c120e49b54885fa07a42b78109569`. The full per-JAR local verification result remains in ignored `run/buja-byte-match.json`; no original JAR, asset or private pack content was added to Git.

The script rejects missing JARs, mismatched names or lengths and same-size content mutations. Added four stdlib `unittest` tests in `scripts/tests/test_verify_buja_snapshot.py`: positive byte equality, same-size byte mutation, missing JAR, mismatched filename. **4/4 tests PASS** and live 65/65 SHA-256 verification PASS. This closes the previously identified archive-vs-folder byte-equality question. It does **not** resolve the literal supplied `D:\buja_server` path (absent on the connected laptop), establish another possible launcher instance does not exist, or validate latest releases and distribution licenses for all 65 projects.
