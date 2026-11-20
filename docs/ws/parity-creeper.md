# WS-7 creeper melee parity, independent slice

## Java 26.3 source

`D:\mc-ref\26.3\src\net\minecraft\world\entity\monster\Creeper.java`, `doHurtTarget(ServerLevel, Entity)`: vanilla returns `true` unconditionally and does not call generic melee damage. Pumpkin had no `CreeperEntity::do_hurt_target` override, so `Mob::do_hurt_target` delegated to `MobEntity::try_attack` and called the target's `damage_with_context` (and could return false). This differs from Java and can damage a player from an attack attempt instead of relying on creeper explosion damage.

## Change

`engine/pumpkin/crates/pumpkin-core/src/entity/mob/creeper.rs`: override `Mob::do_hurt_target` to return `true` without invoking the inherited attack path. Leave standard melee logic for other mobs unchanged; no ABI or scheduler changes.

Test `creeper_melee_attempt_cannot_damage_a_target` creates a creeper and pig in a test world, invokes the mob attack hook and checks the return result and unchanged target health. **RED before fix**: failed at `assert!(creeper_mob.do_hurt_target(pig.as_ref()))` (0 passed, 1 failed). **GREEN after fix**: 1 passed, 0 failed (815 filtered), using Pumpkin's manifest with the outer `pack` profile env and `--target-dir target`. This is a direct-hook unit test backed by the decompiled vanilla method, not a dynamic Java-vs-Rust AI-goal scenario. Engine pack build `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness` PASS; `cargo clippy --manifest-path engine/Cargo.toml --profile pack -p pack-dev --all-targets -- -D warnings` PASS; `cargo fmt --manifest-path engine/pumpkin/Cargo.toml --all --check` PASS. Differential `tests/harness/mobs/creeper-stationary.txt` using the real Java 26.3 server jar vs freshly built Pumpkin PASS: 0 differences, no errors, no allowances, output `run/creeper-stationary-report.json`. The new fixture measures stationary creeper saved state, not dynamic melee-goal behavior.

## Remaining

Vanilla `Creeper.explodeCreeper` also spawns an area effect cloud when a creeper has potion effects and calls additional on-death effects; the Rust implementation still has a TODO for the cloud. Explosion, ignition AI, and on-death behavior need separate testable slices. This commit does not claim full creeper parity.
