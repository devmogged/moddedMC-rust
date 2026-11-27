# Polymorph: native crafting-conflict selection

Status: **implemented early gameplay slice; not a complete Polymorph port**.

## Native path

Pumpkin enumerates the same generated vanilla recipes and current datapack recipes
as ordinary crafting. The default first matching recipe is unchanged. The
appended `moddedmc:crafting` ABI provides bounded ordered matches; the SDK maps
the returned saved ItemStack NBT and known recipe IDs into `RecipeChoices`.
`/polymorph list` displays matching alternatives and `/polymorph choose N`
selects an actual recipe in the active 2×2 or 3×3 crafting screen. The server
updates the result slot in place and uses the same pinned choice for consumed
ingredients and remainders. This works with an unmodified Java 26.3 client.

The menu-local selection validates ingredient identity and components, recipe
identity, result, and remainder pattern before reuse. Ingredient counts may
decrease across successive crafts without changing the active recipe; an
empty ingredient slot or changed stack invalidates the selection. Pumpkin's
recipe manager supplies a generation revision for inexpensive checking of
cached results on datapack reload. Providers without a reliable revision
are revalidated on every result read. Selection is not preserved when a menu
is closed or restarted; this avoids stale index entitlements.

## Deterministic regression evidence

The synthetic **test datapack**, in `tests/scenarios/polymorph-fixtures/conflicts`,
adds two single-stone recipes (diamond and emerald) alongside the normal
vanilla stone-button recipe, and two single-milk-bucket recipes (gold and iron).
It is testing instrumentation, **not** a source release or bundled pack recipe.

The following independent live scenarios use the optimized `pack-dev` engine,
the packaged native Polymorph mod and `moddedmc-bot`, retaining run logs:

- `polymorph-conflicts.txt`: select emerald over vanilla/dynamic alternatives;
  three successive shift-click crafts yield exactly three emeralds, consume
  exactly three stone, never mint diamonds, and survive server restart.
- `polymorph-multiplayer.txt`: Alice picks emerald while Bob picks diamond
  from simultaneous independent menus; invalid selection fails without
  replacing Bob's result. Exact two-item conservation per player.
- `polymorph-player-inventory.txt`: the same selection and conservation on
  the player's 2×2 grid, without opening a crafting table.
- `polymorph-remainders.txt`: choose the iron result of a milk-bucket conflict;
  the bucket remainder stays in the crafting slot, and exactly one iron and
  one bucket reach player inventory. Moving out an ingredient invalidates
  the choice before take.
- `polymorph-reload-invalidates.txt`: removing the selected emerald recipe
  while its original menu is open reverts the cached output to a stone button;
  exactly one button is crafted and no emerald or diamond is created.
- `polymorph-result-cycle.txt`: two right-clicks on the actual result slot
  cycle stone-button to diamond to emerald without moving or consuming
  ingredients; simultaneous Bob remains on default button. Exact output
  quantities checked after both players craft their respective selections.
- `polymorph-cycle-player.txt`: stock 2x2 inventory result cycling chooses
  diamond then emerald, and crafting consumes exactly two stone for two emeralds.

These are **protocol-bot gameplay checks**. Right-click selection needs no
text command and is stock-client compatible; it does not establish the
source mod's original graphical button/arrow model in a graphical client.

## Incomplete parity

- Original Polymorph GUI choice arrows/control and its other recipe handlers
  (stonecutting, smelting, smithing, merchant interfaces where applicable).
- Consistent recipe selection in vanilla and modded automation/crafter blocks;
  the vanilla Crafter still executes its ordinary first-match path.
- More adversarial live tests: recipe reordering, concurrent grid mutation,
  full-inventory edge cases,
  remainder overflow and disconnect/crash boundaries.
- Source-specific persistent choice preferences, if provided in a verified
  upstream implementation; there is no fabricated global preference.

For any reproduction, stage the fixture under a **fresh**
`run/<unique>/world/datapacks/conflicts` directory, then run
`scripts/scenario.ps1 -Scenario tests/scenarios/<scenario>.txt -RunDir
run/<unique> -Port 26180 -Seed 42 -Keep` **sequentially**. The runner itself
does not build the engine or package the mod. Never overwrite retained failure
logs or previously verified run worlds.
