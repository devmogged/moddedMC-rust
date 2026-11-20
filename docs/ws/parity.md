# WS-7 vanilla parity

## M1: Mobs (E5), in progress

Engine commit: `52bbbbf04` (`fix(spawning): check mob obstruction before natural spawn`), on top of the previously published `dddc5a0ff` attribute-sync change. The outer branch records the submodule revision, fixture and this log.

### Natural spawn obstruction

Java 26.3 source: `D:/mc-ref/26.3/src/net/minecraft/world/level/NaturalSpawner.java`, `spawnCategoryForPosition` and `isValidPositionForMob`. Vanilla checks `mob.checkSpawnRules(..., NATURAL)` and `mob.checkSpawnObstruction(level)` after constructing the mob but before `finalizeSpawn` and adding it to the level. `Mob.java` defines the obstruction check and individual mob classes may override it.

Engine source: `engine/pumpkin/crates/pumpkin-core/src/world/natural_spawner.rs`, `spawn_category_for_position` and `is_valid_position_for_mob`. The prior implementation checked the despawn distance only at that stage and treated non-mob types as valid. The change checks the mob obstruction hook before `finalize_spawn` and refuses non-mob types, preserving the prior distance condition. `crate::entity::mob::Mob::check_spawn_obstruction` contains the existing species-specific checks. No host interface or mod ABI was changed.

Regression: `natural_spawns_reject_occupied_mob_space` instantiates a candidate zombie in an otherwise empty test world, then registers a second zombie at the same position and verifies that the candidate is no longer eligible. This checks an actual entity overlap, rather than mirroring the condition's arithmetic.

### Verification

- Engine pack build before and after the source edit: `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness`: PASS on Windows, with local Java 26.3 server jar in `PUMPKIN_MINECRAFT_SERVER_JAR`.
- Focused regression: `cargo test --manifest-path engine/pumpkin/Cargo.toml --target-dir engine/target --profile pack -p pumpkin-core natural_spawns_reject_occupied_mob_space -- --nocapture` with `CARGO_PROFILE_PACK_*` configured to match the pack profile: 1 passed, 809 filtered, 0 failed.
- `cargo fmt --manifest-path engine/pumpkin/Cargo.toml --all --check`: PASS.
- Java 26.3 dynamic oracle: `moddedmc-harness run tests/harness/mobs/zombie-stationary.txt --reference vanilla:D:/mc-ref/26.3/server-bundler.jar --subject target/pack/pack-dev.exe --run-root run/harness-parity-zombie --json`: FAIL with **31 unexplained differences** at the snapshot (0 allows). The reference zombie saves `HurtTime: 0s`, absent from Pumpkin. The reference saves only one explicit attribute (`movement_speed`) while Pumpkin serializes 29 attributes including defaults. The differences are logged in `run/harness-parity-zombie-final/scenario-*/report.json`. Repeated after rebasing on `dddc5a0ff` and rebuilding the combined engine: still exactly 31 differences, zero allowed, no harness runtime error. The fixture probes and captures a deterministic `NoAI` zombie; it does not test stochastic natural spawns.
- `cargo clippy --manifest-path engine/Cargo.toml --profile pack -p pack-dev --all-targets -- -D warnings`: PASS. Java source review validates the obstruction check's placement, but full dynamic natural spawn parity remains unverified.
- Full M1 remains open: mob spawning rules and caps, common mob goals and brains, and loot all require additional differential fixtures. The natural spawn behavior depends on randomness, so no tick-exact spawn outcome or distributional parity is claimed from this source comparison.

### Next

Address the saved-mob differences before claiming the zombie snapshot as parity. Java `AttributeMap.pack()` persists only attribute instances materialized in its lazy map; Pumpkin currently serializes every preallocated instance. `HurtTime` also differs, and requires checking the Java hurt timer lifecycle rather than hardcoding zero. After these are repaired, rerun the same fixture without `allow`. A further source-level discrepancy needs proof and likely a schema change: Java `MobSpawnSettings` selects from weighted spawning lists, while the generated Rust `Spawner` type currently has no weight and `get_random_spawn_mob_at` calls uniform `.choose()`. Carry the weight through data generation before replacing selection, with a distribution test against the original biome spawn tables. Separately build deterministic or distributional tests for natural spawn attempts, then continue through spawning caps, AI and loot. Do not mark M1 done on the current evidence.
