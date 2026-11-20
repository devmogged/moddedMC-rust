# Legendary Monsters — Rust port

Original target: **Legendary Monsters 2.2.4** for **Minecraft 1.21.1 NeoForge**,
published October 23, 2026. Server target: moddedMC-rust / Pumpkin's Minecraft Java 26.3.
The version gap requires explicit behavior tests against the Java original.

## Implemented: M1 passive materials and vanilla guide book (partial)

Exactly **24** source registrations using vanilla Item and default properties or
fireResistant() only, transcribed from the official release's ModItems.java.
The two bones, Warped Mushroom Cap Part, Air Rune and Withered Horn use a
fire-resistant vanilla item carrier; the other nineteen have stack-64 vanilla
carriers. The content item IDs retain the exact original legendary_monsters: namespace
even though the drop-in package ID is legendary-monsters.

These entries have **no custom Rust behavior** because the original registrations
are plain items. Their visual models are referenced by resource location, but the
original models/textures are **not** embedded; they must come from the user's
locally installed original JAR. The 25 model JSON paths (24 materials and one guide book) were verified to exist
in the original 2.2.4 archive.

The 25th item, `legendary_monsters:guide_book`, delegates to the built-in
`minecraft:writable_book` carrier. Its original Java class directly constructs
vanilla `WritableBookItem`; the bot verified two separate one-book slots,
an item-use packet and stack persistence after restart. Editing/signing pages
still requires a real modded-client play test.

Two source-matched recipes and two additive vanilla item tags for Enderitium
are packaged as data. The shapeless ingot-to-gems recipe was executed through a
player inventory (1 ingot produced 9 gems). The shaped nine-gems-to-ingot
recipe was executed through a crafting table (9 gems produced 1 ingot), with
ingredient consumption and restart inventory persistence verified.

Nothing else is complete: custom weapon actions, equipment attributes, food, rare
item metadata, shields, spawn eggs, 46 blocks, remaining recipes and loot, 34 living entities,
bosses, projectiles and worldgen all remain unported. The current generic item
carrier table cannot express every original item property; do not infer Java
parity from item registration or successful Rust compilation.

## Build and check

From the repo root, run:

    cargo fmt --package legendary-monsters --check
    cargo clippy -p legendary-monsters --all-targets -- -D warnings
    cargo test -p legendary-monsters
    powershell -NoProfile -File scripts/pack-mod.ps1 -Crate legendary-monsters

This builds target/mods/legendary-monsters.mmc with a manifest and a platform
library. For runtime verification, use the shared server/bot binaries and run:

    powershell -NoProfile -File scripts/scenario.ps1 -Scenario tests/scenarios/legendary-monsters-materials.txt -RunDir run/legendary-monsters-m1 -Port 26060 -Fresh

Five Rust scenarios **passed** on shared engine d6e5e8d560 (ABI 1):
`legendary-monsters-materials.txt` (registration, recipe availability, grants,
restart); `legendary-monsters-all-items.txt` (24 material IDs in player inventory
before/after restart, twice); `legendary-monsters-guide-book.txt` (book stacking,
use packet and restart); `legendary-monsters-crafting.txt` (2x2 shapeless), and
`legendary-monsters-crafting-table.txt` (3x3 shaped and ingredients consumed).
The original NeoForge 1.21.1 oracle independently confirmed all 24 material IDs
and persisted chest inventory, the guide-book ID, and original lava survival of
five fire-resistant materials versus an ordinary crystal. The original mod's
client-assisted crafting, book UI and most behavior still lack differential
checks. Some content-pack assets render as placeholders.

Optional source-artifact audit (standard Python 3 library only):

    & "$env:LOCALAPPDATA\Programs\Python\Python314\python.exe" mods/legendary-monsters/tools/audit_artifact.py --jar cache/legendary_monsters-2.2.4-MC-1.21.1.jar --decompiled D:/mc-ref/legendary-monsters-2.2.4-decompiled/net/miauczel/legendary_monsters/item/ModItems.java

It requires the exact original JAR hash and compares item constructors, source
translations, 25 model paths, recipe payloads and tag subsets without copying
restricted assets.

See `docs/ws/legendary-monsters.md`, `docs/ws/legendary-monsters-inventory.md`,
`docs/ws/legendary-monsters-blocks.md` and `docs/ws/legendary-monsters-oracle.md`
for version-pinned inventories, Java comparison commands, remaining scope and ABI dependencies.

## Rights

See LICENSE.md. Do not bundle the original restricted assets or
copied unreviewed data files in this public Rust package.
