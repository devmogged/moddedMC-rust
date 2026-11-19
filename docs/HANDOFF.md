# Handoff, 2026-11-19

Where work stopped and how to carry on. Read [ROADMAP.md](ROADMAP.md) and [BACKLOG.md](BACKLOG.md) for the bigger picture. The rules in the engine's `AGENTS.md` apply. Never commit Create's textures, models, sounds or lang files (all rights reserved). Its code and `data/` are MIT.

## Setup

- This repo is `devmogged/moddedMC-rust`, branch `main`. The engine is the submodule `engine/pumpkin` (`devmogged/Pumpkin`, branch `moddedmc`). Run `git submodule update --init` after pulling.
- Build the server and the test bot: `cargo build --profile pack -p pack-dev -p moddedmc-bot` in `engine/`.
- Build the Create mod: `powershell -NoProfile -File scripts/pack-mod.ps1 -Crate create`.
- Run a scenario: `powershell -NoProfile -File scripts/scenario.ps1 -Scenario tests/scenarios/<file>.txt -RunDir run/<name> -Fresh`. The verbs are listed at the top of `scripts/scenario.ps1`.
- Unit tests for Create's kinetics: `cargo test -p create` (seven tests).

## Mod side: Create port (P1)

Source of truth: Create 6.0.10 for MC 1.21.1 NeoForge, tag `mc1.21.1-6.0.10`. Port from that tag only, never from older jars.

**Done and tested:**
- P1a: shafts, cogs and the creative motor.
- P1b: `mods/create/src/kinetics.rs` ports Create's rotation propagation, kinetic networks and stress. It adds the gearbox, clutch, gearshift, encased shaft, casing and turntable. `tests/scenarios/p1b-create.txt` covers it. See the P1 row in BACKLOG.

**In the last commits, not yet run end to end:**

1. **The `moddedmc:entities` v1 mod interface.**
   - Lets a mod hear vanilla `entityInside` for its blocks, read an entity (kind, position, motion, rotation) and change it.
   - Code: `api/src/entities.rs`, `api/src/abi.rs`, and in the host `engine/moddedmc-host/src/content.rs` (`on_entity_collision`, `entity_view`, `entity_change`). Documented in [MOD-ABI.md](MOD-ABI.md).
   - Built, passed Clippy, and a debug run showed it working.
2. **The turntable spinning mobs and items** (`entity_inside` in `mods/create/src/lib.rs`, a port of `TurntableBlock.entityInside`).
   - A debug run showed an armor stand losing 4.8° of yaw per tick at 16 RPM, which is correct.
3. **Engine fix in `engine/pumpkin`:** the selector options `x_rotation` and `y_rotation` were swapped in `entity_selector/parser.rs` (`y_rotation` checked pitch). Vanilla maps x to pitch and y to yaw.
   - The turntable check in `p1b-create.txt` failed only because of this bug.
   - **This fix has not been built yet.**

**Next steps:**
1. Build the engine, rebuild the mod, and run `p1b-create.txt`. The turntable section (`y_rotation=-170..-10`) should pass now.
2. If it does, also run `create-data.txt` and `p1a-create.txt` to check for regressions.
3. Then start P1c, belts. INTERFACES-PLAN section 1 lists the generic operations belts still need:
   - item use on a block,
   - entity queries in a box,
   - item stacks across the interface,
   - container access,
   - display entities for items in transit.

**Known gaps:** listed at the bottom of `mods/create/README.md`. Most visible in game: no spinning shaft halves inside gearboxes, no casing application recipes, no overstress particles.

## Harness side: F2, by the agent "Sol"

Spec: [HARNESS.md](HARNESS.md). It runs the same scenario on a real vanilla 26.3 server and on ours, then diffs the results.

**Done** (commit `38d2f14`):
- `moddedmc-harness run`, `run-all` and `self-test`.
- Tick sync from the game clock, Anvil snapshot comparison, player NBT and console probes.
- Bot place and break synchronised with server acknowledgements.
- Formatting, Clippy and the three harness tests pass.
- Vanilla against itself shows no differences, and a deliberate mutation is caught as exactly one difference.

**Baseline:** [harness-baseline.md](harness-baseline.md) records 11 differences across five scenarios: one repeater timing difference and ten saved-NBT differences. Treat these as known; anything new is a regression or a new finding.

**Left to do:**
- packet comparison with entity-id mapping,
- synchronised move, look, attack and inventory clicks,
- cross-version mod oracles (running Java Create next to ours),
- Linux verification.

Sol's own pause notes are in [harness-status.md](harness-status.md). To resume, use the build and self-test commands in HARNESS.md, then rerun `run-all` against the current `pack-dev`.

## Rules worth knowing

- Java Edition only; Bedrock is on hold.
- Stage files by explicit path, never `git add -A`.
- Use conventional commits with a scope, subject line only.
- Push only to the devmogged repos, never to upstream Pumpkin-MC.
