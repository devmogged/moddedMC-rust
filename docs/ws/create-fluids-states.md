# Create pipe state integration slice

Source: Create 6.0.10, `mc1.21.1-6.0.10`, commit `ac0c444d9828da3453ae8cc65338e8de063286fb` (`FluidPipeBlock.updateBlockState`, `FluidPropagator.getStraightPipeAxis`). Branch `ws/create-fluids-states` is deliberately isolated from the concurrently active `ws/create-fluids` worktree.

## M1a: Connection-state rules

Ported the regular pipe's exact six-face auto-connection behavior into `mods/create/src/fluids/pipe_states.rs`: preserve bracketed connections, skip the ignored neighbor, open the opposite end of a single connection, retain the previous two-face pipe when all neighbors disconnect, and otherwise choose the preferred axis. The straight-axis query accepts only exactly two opposite faces. Direction indices match `kinetics::FACES`: down/up, north/south, west/east.

Verification on Windows: `cargo fmt -p create`, `cargo test -p create --lib` (36 passed, including six new pipe-state cases), `cargo clippy -p create --all-targets -- -D warnings` (pass), `git diff --check` (pass). No native-server scenario or original NeoForge comparison was run for this pure logic slice. Java methods and branch outcomes were inspected directly, not exercised in a Java runtime.

## M1b: Open-ended pipe source-block decisions

Ported the state checks from `OpenEndedPipe.removeFluidFromSpace` and `provideFluidToSpace` into `fluids/open_end.rs`: loaded-only and replaceability gates, existing-source rejection, different-fluid collision, non-block fluid effects, waterlogging, config-controlled source placement, ultra-warm water evaporation, and infinite-source regeneration. Ported `VanillaFluidTargets.drainBlock` to `fluids/vanilla_targets.rs`: water cauldrons drain only at level 3 (1000 mB), lava cauldrons drain 1000 mB, honey drains 250 mB at level 5, and simulation preserves the block. Tests include boundary levels and simulation/execution side-effect separation. These pure plans still require host-side block writes, fluid ticks, side-effects and handlers before in-game verification.

Verification: `cargo fmt -p create`, `cargo test -p create --lib` (46 passed, 10 new cases since M1a), `cargo clippy -p create --all-targets -- -D warnings` (pass), `git diff --check` (pass). Java code was inspected against Create 6.0.10; a Java runtime oracle remains pending.

## M1c: Pipe connection NBT persistence

`fluids/connection_nbt.rs` maps `PipeConnection.serializeNBT/deserializeNBT` to owned fastnbt compounds with Create's direction names, pressure float lists, NeoForge 1.21.1 FluidStack `id`/`amount`/optional `components`, boolean `In`, and the absent/present `Progress` completion convention. Progress saves Catnip `LerpedFloat` fields `Speed`, `Target`, `Value`, keeping unknown chase values when loaded; existing `OpenEnd` NBT remains opaque until the world adapter is present. Empty flow tags omit pressure/progress as Java does. Four focused tests exercise partial- and complete-flow reload, component-preserving NBT, pressure reset, and actual named-root binary round trips.

Reference: NeoForge `FluidStack` (1.21.1), Catnip `LerpedFloat` (Creators-of-Create/Ponder), and Create 6.0.10 `PipeConnection`. Verification: `cargo fmt -p create`, `cargo test -p create --lib` (50 passed), `cargo clippy -p create --all-targets -- -D warnings` (pass), `git diff --check` (pass). The serialized tag structure was compared with source methods, not a captured Java-server NBT oracle. No root-world save/restart scenario has been run for this module.

## M1d: FluidNetwork graph traversal

Added `fluids/network_graph.rs` from Create 6.0.10 `FluidNetwork.tick/reset`. It models the two-tick initial propagation pause, at most 16 BFS propagation cycles per tick, per-face frontier and queued state, component-bearing fluid identity, inward/outward flow completion, endpoint discovery, propagation into connected pipes and retry when a source or chunk is unavailable. Host-neutral world queries remain behind `NetworkTopology`; `network.rs` already has the simulate/execute transfer calculations.

Five new tests verify the two initial waits, 17-connected-pipe 16-cycle boundary, unloaded-chunk recovery, different-fluid branch rejection and retry when endpoint discovery is delayed. Verification on Windows: `cargo fmt -p create`; `cargo test -p create --lib` (55 tests passed); `cargo clippy -p create --all-targets -- -D warnings` (pass); `git diff --check` (pass); `powershell -NoProfile -File scripts/pack-mod.ps1 -Crate create` (packed 314,045-byte .mmc); and `scripts/scenario.ps1 -Scenario tests/scenarios/p1a-create.txt -Port 26028 -RunDir run/ws2a-p1a -Fresh -Exe ..\\..\\mmc-shared\\bin\\d6e5e8d560\\pack-dev.exe -BotExe ..\\..\\mmc-shared\\bin\\d6e5e8d560\\moddedmc-bot.exe` (PASS: pack acceptance, RPMs, cog ratio, motor restart, shaft break; run folder removed). The scenario only checks pre-existing kinetics, **not the new fluid graph**, which is not connected to a fluid world adapter.

## M1e: Source discovery and neighboring pipe supplies

`fluids/flow_source.rs` follows Create 6.0.10 `PipeConnection.determineSource` and `FlowSource.provideFluid` / `OtherPipe.provideFluid`. It refuses unloaded neighboring blocks, distinguishes open-ended pipes from generic sided fluid handlers, prefers a handler's one-mB simulated generic drain before checking individual component-matching tanks, and accepts only completed outward pipe flow. The original extraction predicate ordering, including its decision on an empty generic result, is preserved. Four tests cover precedence, one-mB simulation and tank fallback, no redundant tank reads after a successful generic drain, and the opposing pipe's flow direction/completion gates.

`cargo fmt -p create`, `cargo test -p create --lib` (59 passed), `cargo clippy -p create --all-targets -- -D warnings` and `git diff --check` passed. These are source-comparison unit tests, not a NeoForge server oracle. Real source resolution and transfer will be integrated when the generic host handler interface is published.

The newly added logic does not register a pipe block or change in-game behavior. M1 as defined in `docs/PARALLEL-PLAN.md` remains unfinished: it still needs content states, neighbor events, the block-entity NBT save/reload hookup, water world manipulation, fluid handlers, pump connections, a bot scenario with restart, a Java 6.0.10 oracle, and the 64-pipe benchmark. The host gaps are queued in `N:\mmc-coord\requests\WS-2-01.md` and `WS-2-02.md`. This branch should be integrated into `ws/create-fluids` by the owner/integrator after the active WS-2 worker has checked for overlap.
