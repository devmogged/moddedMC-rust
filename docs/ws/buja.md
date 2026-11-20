# WS-1: buja inventory and small ports

## M0: inventory (provisional, 2026-11-20)

Branch: `ws/buja`. Worktree: `N:\mmc-wt\buja`. Scope is inventory and small independent mods; Create, addons, engine and the ABI belong to other workstreams.

Candidate source: `D:\minecraft\buja_server\mods`, an existing Forge 1.20.1 server directory also referenced by `mods/create/README.md`. The owner has **not yet confirmed** it is the complete current buja pack. Its 65 JARs (the extra `temp.py` is not a JAR) are inventoried in [`docs/BUJA.md`](../BUJA.md). Original JARs and assets remain outside Git.

Evidence: read embedded `META-INF/mods.toml` / `fabric.mod.json`, extracted declared mod IDs, versions, dependencies and license fields, and matched installed SHA-1 against Modrinth. There are 48 exact file-hash matches, 11 additional project-title/mod-ID matches requiring publisher checks, and 6 official CurseForge release-page checks. Each row records its source and the latest published stable release found on that service, with its game version and loader. This is not a claim that all projects have been checked against every distribution site.

Provisional categories: 33 server ports (including Create), 6 engine/vanilla replacements, 8 client-only mods, 14 libraries, 4 data/worldgen mods. Code-license labels are the installed JARs' claims; independent license-file and asset redistribution checks are still needed for every chosen port. Create Recycle Everything has 6 compiled classes, including three custom item classes, and 228 `data/` entries, so it is a small WS-4 addon port, not merely a data pack. Create More Recipes provides custom fan and spouting recipe types, so its runtime must not be skipped.

Verification performed: every JAR was readable as ZIP; exactly 65 unique candidate filenames appear in the source metadata export and inventory. Inventory has 65 mod rows and 65 corresponding manifest entries. No native gameplay scenarios were run: this milestone changes documentation only, and WS-0 has not published shared engine binaries. Real mod parity and asset licenses remain unverified. No Git changes were made in the integrator's checkout.

**Blocker before M0 completion:** owner confirmation that `D:\minecraft\buja_server\mods` is the intended complete buja pack, or identification of the correct instance/export. If corrected, redo inventory and release checks on the authoritative list. Do not mark M0 complete until then.

## M1: port order (draft; not started)

Prioritize by visible player impact, ability to implement actual gameplay through the current ABI, and size. Candidate first substreams for owner-launched separate chats: `WS-1a` Waystones (high visibility, needs teleport/ownership and menu parity), `WS-1b` Corpse (death persistence, item recovery; may need entity and inventory hooks), `WS-1c` Nature's Compass (biome search/world queries), `WS-1d` Explorer's Compass (structure search), `WS-1e` Polymorph (recipe selection UI), `WS-1f` Easy Villagers (villager interactions and workstations), `WS-1g` Lootr (per-player container loot), `WS-1h` Simple Storage Network (inventory network and menus), and `WS-1i` Musket Mod (projectiles and weapon events). These are provisional; versions, licensing, source methods and ABI requirements must be rechecked when each starts. Do not spawn any of these parallel workstreams from WS-1; the owner starts them.

L/XL follow later: Farmer's Delight, Epic Knights, Small Ships, Sophisticated storage/backpacks, Quark, Refined Storage, Recruits, Alex's Mobs and Alex's Caves. Create and its addons, dragons, Cataclysm and Legendary Monsters are owned by their other designated workstreams. No standalone S-sized WS-1 candidate has passed its source/ABI survey; do not manufacture a placeholder mod.

## Next

Confirm authoritative buja source; correct and complete M0 license/source checks; review M1 prioritization with real ABI gaps; then start one independent S-sized mod (if one is found), adding genuine tests, scenario, original-mod comparison and benchmark where applicable. Use ports 26010-26019. Ask WS-0 for host gaps, never implement them in this branch.

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
