# Legendary Monsters — Rust port

Original target: **Legendary Monsters 2.2.4** for **Minecraft 1.21.1 NeoForge**,
published October 23, 2026. Server target: moddedMC-rust / Pumpkin's Minecraft Java 26.3.
The version gap requires explicit behavior tests against the Java original.

## Implemented: M1 passive materials (partial)

Exactly **24** source registrations using vanilla Item and default properties or
fireResistant() only, transcribed from the official release's ModItems.java.
The two bones, Warped Mushroom Cap Part, Air Rune and Withered Horn use a
fire-resistant vanilla item carrier; the other nineteen have stack-64 vanilla
carriers. Two additional passive items, Portal Shard and Eye Crystal, use the
item-defaults ABI to preserve epic rarity and the amethyst shard carrier's
64-item stack limit. The content item IDs retain the exact original legendary_monsters: namespace
even though the drop-in package ID is legendary-monsters.

These entries have **no custom Rust behavior** because the original registrations
are plain items. Their visual models are referenced by resource location, but the
original models/textures are **not** embedded; they must come from the user's
locally installed original JAR. All 26 model JSON paths were verified to exist
in the original 2.2.4 archive.

Two source-matched Enderitium recipes and two additive vanilla item tags
are packaged as data. The shapeless ingot-to-gems recipe was executed through a
player inventory (1 ingot produced 9 gems). The shaped nine-gems-to-ingot
recipe was executed through a crafting table (9 gems produced 1 ingot), with
ingredient consumption and restart inventory persistence verified.

The two source-matched salvage recipes turn a Knight's Sword Blade Half into
one iron ingot and a Long Stick Half into two sticks. The salvage scenario
passed both transactions and inventory persistence.

The Enderitium sword and pickaxe now have working vanilla-compatible equipment
components and diamond/netherite smithing upgrades. The survival
`legendary-monsters-combat-repair.txt` scenario passed against the previously
compiled November 27 development server and package: diamond ore mining yielded
one diamond and one durability damage; a sword critical hit reduced a ravager's
health from 100 to 88.75 and consumed one durability. An anvil rejected a
diamond, then consumed exactly one Enderitium ingot to reduce a damaged pickaxe
from 1000 to 388 damage (2450 maximum). The repaired item and remaining ingot
survived a full restart. This is protocol-bot evidence, not a graphical test.

Nothing else is complete: custom weapon actions, equipment attributes, food,
other item metadata, shields, spawn eggs, 46 blocks, remaining recipes and loot, 34 living entities,
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

All three scenarios **passed** on shared engine d6e5e8d560 (ABI 1):
legendary-monsters-materials.txt verifies registration, recipe availability,
/give and restart, legendary-monsters-crafting.txt executes the shapeless
recipe, and legendary-monsters-crafting-table.txt executes the shaped recipe
with save/restart and consumed-ingredient checks. Some content-pack
assets still render as placeholders. Runtime parity against the original
NeoForge 2.2.4 mod is **not yet established**.

The salvage scenario also passed on that shared binary. The epic-materials
scenario requires a newer server exposing `moddedmc:item_defaults` v1 and
passed on the November 26 bounded-slabs test binary: both 65-item grants
split into 64 + 1 and survived restart. Verify purple item names in a real
stock client; the headless inventory test does not inspect tooltip colors.

Optional source-artifact audit (standard Python 3 library only):

    & "$env:LOCALAPPDATA\Programs\Python\Python314\python.exe" mods/legendary-monsters/tools/audit_artifact.py --jar cache/legendary_monsters-2.2.4-MC-1.21.1.jar --decompiled D:/mc-ref/legendary-monsters-2.2.4-decompiled/net/miauczel/legendary_monsters/item/ModItems.java

It requires the exact original JAR hash and compares item constructors, model
paths, recipe payloads and tag subsets without copying restricted assets.

See docs/ws/legendary-monsters.md and docs/ws/legendary-monsters-inventory.md
for the full version-pinned inventory, constraints, remaining scope and ABI dependencies.

## Rights

See LICENSE.md. Do not bundle the original restricted assets or
copied unreviewed data files in this public Rust package.
