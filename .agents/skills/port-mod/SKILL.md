---
name: port-mod
description: How to port a Java Minecraft mod from the buja pack to a drop-in Rust .mmc mod on moddedMC-rust, from the source study through to scenarios and an in-game check. Use when starting or continuing any mod port, or when adding host or engine support a port needs.
---

# Port a mod

A port is a faithful copy of the mod's behaviour that players can't tell apart from the Java mod,
running as a drop-in `.mmc` on our Pumpkin fork and much faster and lighter. Functionality comes
first: anything a player can see or hit gets done now. Only differences nobody can notice may wait,
and only as an issue labelled `deferred-parity`.

## Ground truth

- Port the mod's latest release (docs/BUJA.md has the "Latest" column; recheck it on Modrinth or
  CurseForge). Work from that release's source and jar, never from memory, the wiki or the old
  pack jar. Keep reference sources and jars outside the repo, in `cache/` or another directory.
- Vanilla behaviour is the decompiled 26.3 server jar (the root AGENTS.md of `engine/pumpkin`
  explains how to get it).
- Check the licence before copying assets. Assets that can't be redistributed stay out of git and
  are read from the local original jar, as Create's are.

## Where things go

docs/PORTS.md maps every Java part (blocks, block entities, ticks, items, data, commands) to its
place in a port. docs/MOD-ABI.md and `api/` are the SDK. `scripts/new-mod.ps1` scaffolds a crate
that already builds and runs. Larger ports to copy from are `mods/create`, `mods/farmersdelight`
and `mods/saints-dragons`.

When the port needs something the host can't do yet (entities, menus, worldgen and so on), check
docs/INTERFACES-PLAN.md and open issues first. Otherwise add it: a generic operation in `host/`
and the engine fork, appended to the ABI and documented in docs/MOD-ABI.md, in the engine's
Pumpkin style. Never fake a mechanic to dodge a missing operation.

## Order of work

1. Study the source: list the mod's content and features with the Java classes behind each, ranked
   by how often players run into them. Put that list in the issue.
2. Copy all its data unchanged (recipes, loot, tags) and register every block and item, so the mod
   loads with everything present.
3. Port mechanics in the order of that list, following each one's whole Java call chain. Commit
   and push every working stage.
4. For each mechanic, add a scenario in `tests/scenarios/` that plays it the way a survival
   player would, including a server restart where state persists. See docs/HARNESS.md. Fix the
   cause when a scenario fails. Don't weaken the scenario.
5. Measure hot paths against the Java mod when the mod has one (docs/PORTS.md, "Measure against
   the original").

## Done

Every feature on the list works or has a deferred issue, every scenario passes, existing scenarios
still pass, and the issue holds a real-client checklist ("do X, expect Y") for what bots can't
see. Clippy and tests are clean for the crates you touched.
