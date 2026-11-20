# Legendary Monsters — Rust port

Original target: **Legendary Monsters 2.2.4** for **Minecraft 1.21.1 NeoForge**,
published October 23, 2026. Server target: moddedMC-rust / Pumpkin's Minecraft Java 26.3.
The version gap requires explicit behavior tests against the Java original.

## Implemented: M1 passive materials (partial)

Exactly **24** source registrations using vanilla Item and default properties or
fireResistant() only, transcribed from the official release's ModItems.java.
The two bones, Warped Mushroom Cap Part, Air Rune and Withered Horn use a
fire-resistant vanilla item carrier; the other nineteen have stack-64 vanilla
carriers. The content item IDs retain the exact original legendary_monsters: namespace
even though the drop-in package ID is legendary-monsters.

These entries have **no custom Rust behavior** because the original registrations
are plain items. Their visual models are referenced by resource location, but the
original models/textures are **not** embedded; they must come from the user's
locally installed original JAR. The 24 model JSON paths were verified to exist
in the original 2.2.4 archive.

Nothing else is complete: custom weapon actions, equipment attributes, food, rare
item metadata, shields, spawn eggs, 46 blocks, recipes, loot, 34 living entities,
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

The scenario checks item registration, stack commands and player inventory across
a restart; its runtime parity against the original mod is **not yet established**.

See docs/ws/legendary-monsters.md and docs/ws/legendary-monsters-inventory.md
for the full version-pinned inventory, constraints, remaining scope and ABI dependencies.

## Rights

See LICENSE.md. Do not bundle the original restricted assets or
copied unreviewed data files in this public Rust package.
