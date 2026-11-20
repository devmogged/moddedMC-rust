# WS-7 vanilla area-effect cloud application parity

Scope: isolated `ws/parity-spawn-empty` branch, `engine/pumpkin/crates/pumpkin-core/src/entity/area_effect_cloud.rs` and `item/potion.rs`. No native ABI changes; active WS-7 worktree untouched.

## Decompiled Java 26.3 source

`D:\mc-ref\26.3\src\net\minecraft\world\entity\AreaEffectCloud.java`, `serverTick` applies effects only on `tickCount % 5 == 0` after `waitTime`. Radius motion is updated every tick only when `radiusPerTick != 0`, otherwise no radius metadata update. Its entity bounding box is `radius*2` wide, **0.5 block high**, with a final horizontal-only `dxÃ‚Â² + dzÃ‚Â² <= radiusÃ‚Â²` center-distance check. Ordinary potion effects have the durations already scaled via `PotionContents.forEachEffect(allEffects::add, potionDurationScale)`; cloud application does not scale duration again by proximity, and instantaneous effects use constant 0.5 potency. Creeper clouds have `potionDurationScale=0.25`; ordinary lingering potions default to 1.0.

Rust previously scanned and applied cloud effects **every tick**, sent redundant radius metadata even if `radiusOnTick=0`, built a spherical rather than 0.5-block-high cloud range, applied a distance attenuation to every effect, and `PotionApplicationSource::AreaEffectCloud` applied an extra 0.25 duration scale on top of the already pre-scaled creeper effects.

## Red Java-vs-Rust differential fixtures

`tests/harness/mobs/area-effect-cloud-application-cadence.txt`: spawn speed-infused creeper and detonate, place an invulnerable pig into its cloud after the wait period at an off-boundary tick. Java's pig has no effect yet; Pumpkin prematurely added speed (one differential), report ignored at `run/cloud-cadence-red.json`.

`tests/harness/mobs/area-effect-cloud-effect-duration.txt`: same cloud with a pig inside for the first application boundary. After a fixed number of ticks Java's pig has 46 remaining speed ticks, while Pumpkin had 8 (one differential), report ignored at `run/cloud-duration-red.json`. This exposed double duration scaling as well as early timing.

## Implementation and verification

`AreaEffectCloudEntity::tick` now gates effect application to ages divisible by five after waiting, keeps radius updates every tick only when the delta is nonzero, uses a half-block-height bounding box and horizontal distance threshold, and passes full pre-scaled effects without proximity attenuation. The existing `PotionApplicationSource::AreaEffectCloud` now retains prepared durations and uses fixed instantaneous potency 0.5. Added focused pure tests for application tick boundaries, horizontal radius and potion-scale semantics. Dynamic fixture, combined suite, pack build, Clippy, and performance outcomes will be appended after validation.

## Remaining caveats

The existing cloud effect eligibility check skips entities that already have an effect; Java instead checks `canBeAffected` and can refresh active effects after the reapplication delay. This is a known follow-up, not silently treated as full cloud parity. Cloud `Owner`, custom particles and potion-component NBT and cloud cooldown persistence remain separate issues.

## Session 2026-11-20

Engine implementation committed as e9c6f20a2 on ws/parity-spawn-empty, then merged origin/moddedmc as 89b7a91d6. The earlier ws/parity mob-state work, including 1f6e34619 attribute and HurtTime fix, is an ancestor of this branch. Outer main was merged into this worktree as ea4f278, pinning 89b7a91d6. The engine branch and outer merge commit were both pushed successfully. The cloud effect timing and duration fixtures below are the acceptance criteria; test/build results are pending and will be appended only after direct execution. Formatting (cargo fmt --manifest-path engine/pumpkin/Cargo.toml --all --check) and git diff --check passed before the engine commit.

## Verified final evidence (2026-11-20)

Full engine build passed: cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness. Three focused pumpkin-core unit tests passed: five-tick application boundary, horizontal radius, and pre-scaled duration / fixed instant potency. Rustfmt --check and git diff --check passed on engine.

Real vanilla Java 26.3 versus freshly rebuilt pack-dev: moddedmc-harness run-all tests/harness/mobs --jobs 2: 7 of 7 scenarios passed, zero differences, zero allowed differences, zero runtime errors. Newly added cloud-application-cadence and cloud-effect-duration both pass; area-effect-cloud-default-lifetime, area-effect-cloud-nbt, creeper-lingering-cloud, creeper-stationary and zombie-stationary also pass. In particular the old zombie-stationary 31-difference blocker has been eliminated by the correctly pinned engine history. Ignored report run/ws7-cloud-validation.

The cloud victim eligibility/reapplication behavior, Owner/custom-particle and potion NBT, generic AI/loot/natural-spawn distribution, and real-client rendering remain outside this verified slice. The broader harness suite is a separate regression gate and must not be interpreted as full gameplay parity.

## Full harness regression (2026-11-20)

Full 25-scenario Java 26.3 differential sweep: 19 passed without differences or allowances, 6 failed in previously known categories with unchanged counts: basic (2), bot-actions (7), bot-place-break (2), client-effects (15), piston-push (6), repeater-chain (1). No harness runtime errors. All seven mob scenarios pass. Ignored report run/ws7-full-validation-summary.json and per-scenario run/ws7-full-validation. This is not a claim of full parity.
