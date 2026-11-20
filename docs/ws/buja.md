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
