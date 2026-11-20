# WS-7 native area-effect-cloud NBT parity

Isolated worktree/branch `N:\mmc-wt\parity-riding`, `ws/parity-spawn-empty`, after vanilla cloud lifetime and creeper lingering effect fixes. No native-mod ABI change; active WS-7 worktree untouched.

## Java 26.3 oracle

`D:\mc-ref\26.3\src\net\minecraft\world\entity\AreaEffectCloud.java`, `readAdditionalSaveData` and `addAdditionalSaveData`: the entity reads/writes `Age`, `Duration`, `WaitTime`, `ReapplicationDelay`, `DurationOnUse` (integers), and `RadiusOnUse`, `RadiusPerTick`, `Radius` (floats). Defaults 0/-1/20/20/0/0/0/3. Java's base `Entity` writes and loads double `fall_distance` even for non-living entities; Pumpkin previously wrote it only from `LivingEntity`.

## Reproduction and correction

`tests/harness/mobs/area-effect-cloud-nbt.txt` summons a cloud with nondefault timing and radius values, probes `Duration`, and snapshots the saved NBT. PRE-FIX real Java-vs-Pumpkin comparison: 10 differences (missing eight cloud fields, missing base fall_distance, and probe failure). Cloud NBT read/write on `AreaEffectCloudEntity` now reuses the existing `EntityBase` virtual methods with the precise Java field names and default values. The primitive fields are included in `/summon`, saving, and reloads, without special code in commands or the differential harness.

After the cloud fields were added, the same fixture had only one remaining difference: `fall_distance` missing for this non-living entity. In Pumpkin's generic `Entity`, a private `non_living_fall_distance: AtomicCell<f64>` now round-trips Java's base double field. `LivingEntity` maintains its existing f32 fall physics and writes its real distance after the base serialization, so no living behavior is replaced. The new field is used only for base serialization/import on entities that do not track fall distance through `LivingEntity`. This does not claim that non-living fall physics is fully modeled.

Unit test `cloud_nbt_roundtrips_vanilla_timing_and_radius_fields` creates a cloud, reads a Java-shape NBT compound including nonzero double `fall_distance`, writes complete EntityBase NBT and checks each field precisely. **PASS: 3/3 area-effect-cloud Rust tests** (818 other tests filtered) including primitive NBT round-trip with nonzero `fall_distance`, lifetime bounds and default radius. Full engine pack build `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness` PASS. Rebuilt Java26.3 dynamic oracle `tests/harness/mobs/area-effect-cloud-nbt.txt` PASS **0 differences, 0 runtime errors, no allowances**, down from 10 pre-fix to one after cloud fields only to zero after adding the base Entity field; reports ignored at `run/cloud-nbt-red.json`, `run/cloud-nbt-after.json`, `run/cloud-nbt-final.json`. Strict `cargo clippy --manifest-path engine/Cargo.toml --profile pack -p pack-dev --all-targets -- -D warnings` PASS; formatting and `git diff --check` PASS. Expanded full-harness regression after the follow-up cloud-cadence fix remains to be run.

## Limits

Cloud `Owner`, `custom_particle`, `potion_contents`, and `potion_duration_scale` NBT are separate vanilla fields not implemented in this primitive slice. Generic cloud effect application timing/geometry also remains an open parity item. The eight primitive fields and base fall_distance do not imply complete cloud serialization or Minecraft parity.
