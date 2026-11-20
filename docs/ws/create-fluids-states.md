# Create pipe state integration slice

Source: Create 6.0.10, `mc1.21.1-6.0.10`, commit `ac0c444d9828da3453ae8cc65338e8de063286fb` (`FluidPipeBlock.updateBlockState`, `FluidPropagator.getStraightPipeAxis`). Branch `ws/create-fluids-states` is deliberately isolated from the concurrently active `ws/create-fluids` worktree.

## M1a: Connection-state rules

Ported the regular pipe's exact six-face auto-connection behavior into `mods/create/src/fluids/pipe_states.rs`: preserve bracketed connections, skip the ignored neighbor, open the opposite end of a single connection, retain the previous two-face pipe when all neighbors disconnect, and otherwise choose the preferred axis. The straight-axis query accepts only exactly two opposite faces. Direction indices match `kinetics::FACES`: down/up, north/south, west/east.

Verification on Windows: `cargo fmt -p create`, `cargo test -p create --lib` (36 passed, including six new pipe-state cases), `cargo clippy -p create --all-targets -- -D warnings` (pass), `git diff --check` (pass). No native-server scenario or original NeoForge comparison was run for this pure logic slice. Java methods and branch outcomes were inspected directly, not exercised in a Java runtime.

The newly added logic does not register a pipe block or change in-game behavior. M1 as defined in `docs/PARALLEL-PLAN.md` remains unfinished: it still needs content states, neighbor events, NBT persistence, water world manipulation, fluid handlers, pump connections, a bot scenario with restart, a Java 6.0.10 oracle, and the 64-pipe benchmark. The host gaps are queued in `N:\mmc-coord\requests\WS-2-01.md` and `WS-2-02.md`. This branch should be integrated into `ws/create-fluids` by the owner/integrator after the active WS-2 worker has checked for overlap.
