# Legendary Monsters 2.2.4 — Java oracle evidence

## Reproducible original server

Exact published file: `legendary_monsters-2.2.4-MC-1.21.1.jar` (44,757,511 bytes), SHA-256 `5A659453D7F439F36A1CDF0CB74EB29CFA98D2B6C693424157E2B67ECB16E136`.
Original Minecraft **1.21.1**, **NeoForge 21.1.219**, JRE **Eclipse Adoptium OpenJDK 21.0.13+9-LTS**. No client launched. Installed isolated at `D:\mc-ref\legendary-monsters-2.2.4-oracle`, with the original JAR in its `mods/` directory. Localhost-only (`127.0.0.1:26066`), offline test server (not public), creative defaults and `level-name=world-oracle`.

From the isolated directory, use PowerShell:

```powershell
& 'D:\mc-ref\legendary-monsters-2.2.4-oracle\jre21\jdk-21.0.13+9-jre\bin\java.exe' -Xms512M -Xmx3G '@libraries/net/neoforged/neoforge/21.1.219/win_args.txt' nogui
```

A Java **26** attempt caused an original Mixin `PlayerMixin` failure (`ClassNotFoundException: java.lang.Boolean` during transformation), so use Java **21**. On Java 21 the exact original JAR started normally and reported `Done (7.165s)`; after a full `save-all`, `stop`, and restart it reported `Done (1.512s)`. The JAR's internal mod version displays `1.21.1`, despite the downloaded artifact file being release 2.2.4; the pinned SHA and file ID distinguish the artifact.

The original server reports a client-only `CameraInvoker` Mixin target absent on a dedicated server and a `legendary_monsters:needs_dinosaur_bone_tool` tag warning. These were **original-mod** observations, not Rust regressions.

## Java console oracle — direct outputs

### All 24 selected item IDs and world save/restart

Run `forceload add 0 0`, `setblock 0 100 2 minecraft:chest`; for each of the 24 registered Rust material IDs from `mods/legendary-monsters/src/lib.rs`, run:

```text
item replace block 0 100 2 container.<slot 0..23> with legendary_monsters:<item_id> 1
```

All **24** replacements returned `Replaced a slot ...` in Java. `data get block 0 100 2 Items` returned 24 distinct slots 0–23, each with `count: 1` and its exact `legendary_monsters:` identifier. The original did not resolve display-name translations for `falling_cloud_projectile` and `falling_cloud_small_projectile`, but their IDs and saved item counts were valid.

After `save-all`, `stop`, Java 21 server restart, `data get block 0 100 2 Items` returned the same 24 IDs/counts in all 24 slots. This verifies Java item recognition and **Java-side chest persistence** for the exact set; it does *not* establish parity of all item components, dropped items or crafting transactions.

### Writable guide book (additional original vanilla-backed item)

Exact original 2.2.4 `ModItems.GUIDE_BOOK` uses `new WritableBookItem(new Properties())`, rather than a mod-specific item subclass. Original `en_us.json` resolves `item.legendary_monsters.guide_book` to **Guide Book**, and the JAR contains its item model. Java NeoForge 1.21.1 accepted `item replace block 0 100 2 container.24 with legendary_monsters:guide_book 1`, and `/data get block 0 100 2 Items` confirmed its original registry ID in the chest. The operator command also accepted a count of 2 in slot 25, which demonstrates that `/item replace` can bypass normal stack-limit checks; **do not use console replacement as evidence of legal player stacking**. The server saved cleanly after both operations.

Rust Pumpkin 26.3 registers `legendary_monsters:guide_book` with the vanilla `minecraft:writable_book` carrier. In the bot scenario `legendary-monsters-guide-book.txt` on port **26065**, giving 2 generated **two separate 1-book slots**, an actual `use-item main` packet was accepted, and both IDs/counts survived a full server restart. This establishes registration, carrier stack limit, protocol use acceptance and persistence only. The actual book editor GUI, writing, signing, and multiplayer book data compatibility have not been tested with a modded real client; these are not yet parity claims.

### Fire resistance in lava

In the force-loaded chunk, set stone under two separate lava source blocks at `(8,100,8)` and `(13,100,8)`, then issue:

```text
summon minecraft:item 8.5 101.2 8.5 {Item:{id:"legendary_monsters:dinosaur_bone",count:1}}
summon minecraft:item 13.5 101.2 8.5 {Item:{id:"legendary_monsters:chorus_crystal",count:1}}
```

After at least 15 seconds, these exact Java commands print the corresponding success string:

```text
execute if entity @e[type=minecraft:item,nbt={Item:{id:"legendary_monsters:dinosaur_bone"}}] run say bone_survived_lava
execute unless entity @e[type=minecraft:item,nbt={Item:{id:"legendary_monsters:chorus_crystal"}}] run say crystal_destroyed_by_lava
```

Then individually summon the remaining four fire-resistant items into the same lava: `withered_bone`, `warped_mushroom_cap_part`, `air_rune` and `withered_horn`. All four were found surviving more than 15 seconds later by `execute if entity @e[type=minecraft:item,nbt={Item:{id:"..."}}] run say survives_<id>`. Thus all **five** `new Item(new Properties().fireResistant())` materials survived and the default-properties Chorus Crystal was destroyed in the **original** server, as expected from the source.

**Rust comparison:** the Rust port uses `minecraft:netherite_ingot` carriers for all five source-fire-resistant items and ordinary carriers for default materials. Constructor/property selection is confirmed by the pinned JAR audit. The original `/summon minecraft:item` and NBT-selector commands remain unsupported on the Rust host; the original selector-based attempt was therefore inconclusive. Two physical-drop bot scenarios supersede that limitation for **item-survival outcomes**: `legendary-monsters-lava-materials.txt` (Dinosaur Bone, vulnerable Chorus Crystal and dry control) and `legendary-monsters-fireproof-all.txt` (all five source-resistant materials, vulnerable crystal and dry control). The expanded scenario **passed** on port 26068 on 2026-11-20: one live item entity persisted in each of five separate lava basins after 12 seconds, zero crystal entities remained in the lava basin, and one crystal entity survived dry stone. Each source inventory slot was checked empty after its physical drop. The Java 2.2.4 server independently confirmed the same fire-survival classification for all six original IDs. Entity counts do not prove rendering, full item-component parity, or NBT-selector support.

### Original Spiky Worm spawn and attributes

Commands on the Java server:

```text
summon legendary_monsters:spiky_bug 0.5 102 0.5 {NoAI:1b,PersistenceRequired:1b}
data get entity @e[type=legendary_monsters:spiky_bug,limit=1] Health
attribute @e[type=legendary_monsters:spiky_bug,limit=1] minecraft:generic.max_health get
attribute @e[type=legendary_monsters:spiky_bug,limit=1] minecraft:generic.armor get
attribute @e[type=legendary_monsters:spiky_bug,limit=1] minecraft:generic.movement_speed get
attribute @e[type=legendary_monsters:spiky_bug,limit=1] minecraft:generic.attack_damage get
attribute @e[type=legendary_monsters:spiky_bug,limit=1] minecraft:generic.follow_range get
```

Console outputs: `Spiky Worm` summoned, `Health=7.0f`, `max_health=7.0`, `armor=6.0`, `movement_speed=0.10499999672174454`, `attack_damage=3.0`, `follow_range≈14.435954711281912` observed (this last queried value is distinct from the older source's base follow-range 14). After Java full save/restart the persistent `spiky_bug` was still there with health 7. Damage/AI attack behavior has **not** been verified. Java 1.21.1 uses `minecraft:generic.*` attribute identifiers; Minecraft 26.3 uses renamed IDs, so comparisons must be semantic and version-aware.

## What cannot yet be claimed

- A headless vanilla Mineflayer 1.21.1 bot was **rejected** by the original NeoForge handshake: `neoforge.network.negotiation.failure.vanilla.client.not_supported`. No original-player crafting, client inventory or attack scenario was run. Original recipe JSON matches all four registered Rust recipes exactly (two Enderitium conversions and two passive-component recycling conversions), but original **gameplay crafting parity** remains untested.
- Seven passing Rust-side gameplay bot scenarios on Pumpkin 26.3 verify the 24-material registry (including two independent full-inventory checks), the vanilla-backed guide book, ingot/gem crafting in 2×2 and 3×3, inventory persistence, and a **controlled real-entity lava comparison**. The lava scenario `tests/scenarios/legendary-monsters-lava-materials.txt` gives and physically drops one source-fire-resistant Dinosaur Bone onto a lava basin and a default-properties Chorus Crystal into another basin. The bot checks the hand slot becomes empty, then checks after 12 seconds: one Dinosaur Bone item entity remains near its basin; zero crystal item entities remain near the lava basin. The **same Chorus Crystal dropped over dry stone** remains as a live item entity after 12 seconds. Two early three-control scratch runs passed, but a later terrain seed exposed false positives and item drift. The permanent scenario was corrected to clear three isolated arenas, lay controlled stone/lava floors, set the bot's facing and verify empty inventory slots after each physical drop. The corrected isolated scenario passed locally on port 26067. It is behavioral evidence for the two tested items, not proof of every item property or five-item coverage. No original Java client-assisted crafting differential was completed.
- The original JAR has missing texture paths for two technical `falling_cloud*_projectile` item model JSONs, both referencing nonexistent `legendary_monsters:item/falling_cloud_projectile` texture; these were confirmed by inspecting JAR model dependencies. The Rust resource-pack warning about placeholders is consistent with this defect but the full warning cause was not proved. Do not redistribute or synthesize original proprietary textures.
- No owner-client play test and no M2/M3/M4 gameplay parity. Original NeoForge service runs are stopped gracefully; no persistent server left running.
