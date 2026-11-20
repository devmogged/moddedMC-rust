# WS-7 independent spawn-block parity slice

Worktree `N:\mmc-wt\parity-riding`; outer and nested engine branches `ws/parity-spawn-empty`, forked from the published WS-7 branches. The original `N:\mmc-wt\parity` worktree is active in another chat and was not edited. This branch is for independent review/cherry-pick, not a replacement of WS-7 mainline.

## Vanilla Java 26.3 source

`D:\mc-ref\26.3\src\net\minecraft\world\level\NaturalSpawner.java`, `isValidEmptySpawnBlock`: rejects a full collision shape, a signal source (`BlockState.isSignalSource()` independent of powered/facing), any nonempty `FluidState`, `PREVENT_MOB_SPAWNING_INSIDE`, and blocks dangerous to the entity. Our prior Rust `is_valid_empty_spawn_block` only rejected `state.is_full_cube()` and `state.is_liquid()` before tag/danger checks. This permitted directional unpowered redstone sources and waterlogged non-liquid block states.

Change to `crates/pumpkin-core/src/world/natural_spawner.rs`: consult the existing block registry `emits_redstone_power` across all six directions, because the registry's directional helper differs from Java's nondirectional source predicate. Check the fluid family from `World::get_fluid_from_state_id(state.id)`, which handles waterlogging. The same check now applies in live natural spawning, chunk-generation spawning, phantom attempts and iron-golem obstruction. The existing full-collision-shape approximation has not been changed, so this is a subset of the full vanilla predicate.

Unit regression `spawn_empty_blocks_match_vanilla_signal_and_fluid_rules`: air allowed, unpowered lever/redstone torch/repeater rejected, dry partial oak slab allowed, waterlogged partial oak slab rejected. **PASS: 1 passed, 0 failed, 814 filtered**, using Pumpkin's own manifest with `--target-dir target --profile pack`, and pack profile values inherited from the outer workspace. `cargo fmt --manifest-path engine/pumpkin/Cargo.toml --all --check` PASS; `git diff --check` PASS. Engine `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness` PASS after the change. Java 26.3 `run-all tests/harness/mobs` with the newly built engine PASS (zombie-stationary: 0 differences, no error, no allowances), recorded in `run/harness-parity-spawn-after.json`. `cargo clippy --manifest-path engine/Cargo.toml --profile pack -p pack-dev --all-targets -- -D warnings` PASS. No mod ABI changes.

## Existing differential harness baseline

On the current built engine, `horse-attributes-on-track.txt` and `bot-riding.txt` each pass Java 26.3 with zero differences. The 19-fixture `run-all tests/harness --jobs 2` sweep reports 13 passing and six failing: basic (2 diffs), bot-actions (7), bot-place-break (2), client-effects (15), piston-push (6), repeater-chain (1). The pre-change JSON report is `run/parity-riding-sweep.json`. The **post-change full 19-fixture sweep** with rebuilt binaries yielded the same 13 passing scenarios and the same six failing scenarios with identical difference counts (2, 7, 2, 15, 6, 1), with zero newly failing scenarios and no harness runtime errors. The complete post-change JSON is `run/harness-parity-spawn-full-after.json`. No allowances were added.

## Remaining validation

A deterministic Java-vs-Rust dynamic fixture for stochastic natural-spawn selection has not yet been established; this slice is proven by matching 26.3 predicate source and the unit regression, not by asserting stochastic world parity. Preserve this distinction in integration. Benchmark natural spawn attempt path if this becomes a measurable hotspot. Further M1 work (AI/loot/caps) continues elsewhere.
