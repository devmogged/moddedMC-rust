# E2: datapack worldgen

Terralith, Tectonic, Incendium and Nullscape ship as worldgen datapacks. Pumpkin generates only
vanilla terrain today: codegen in `tools/pumpkin-codegen` reads the vanilla datapack in `assets/`
and emits Rust (`noise_router.rs`, `noise_settings.rs`, `material_rule.rs`, `biome.rs`,
`configured_feature.rs`, `placed_feature.rs`, `carver.rs`, `structures.rs`). Datapack worldgen
files are already loaded by `dynamic_registry_loader.rs`, but only for client registry sync.

The path in is that generation already runs on data, not on generated code:
`ProtoNoiseRouters::generate(&BaseNoiseRouters, &GlobalRandomConfig)` takes the component stacks
codegen emits. Building those same structures at runtime from the merged datapack JSON leaves the
hot path untouched.

## Rules

- Each stage proves itself against vanilla, not against Pumpkin: first that the runtime loader
  rebuilds vanilla's own data identically to codegen (cheap, exact), then chunk dumps from a real
  vanilla 26.3 server with the same datapack and seed through the F2 harness.
- Codegen and the runtime loader share one parser. Move the serde models and resolution logic out
  of codegen into a crate both can use, instead of writing a second parser.
- Static data that stays static keeps its `&'static` form. Runtime data is built once per
  datapack load and leaked or held in an `Arc`; no allocation or locking per sampled block.
- Generation speed is measured on `benches/` before and after every stage, vanilla world, same
  seed. A slowdown on vanilla terrain is a bug.
- Unsupported entries (a feature type Pumpkin lacks, a density function it can't build) are
  logged once with the id and skipped, never a panic.
- Port the latest release of each datapack for 26.3; record versions and download sources in
  `docs/E2-PLAN.md` as each stage starts using them.

## Stages

Commit each stage separately with its tests.

| Stage | Scope | Proof |
| --- | --- | --- |
| E2a | Density functions and noise settings. A runtime builder that turns `worldgen/noise_settings` and `worldgen/density_function` JSON (references resolved across packs, highest priority wins) into `BaseNoiseRouters`, used for the dimension's generator when a datapack overrides any of them. | The runtime builder fed vanilla's JSON yields component stacks equal to the generated `OVERWORLD_BASE_NOISE_ROUTER` (and nether, end). `proto_chunk_test` passes with the runtime router. Tectonic's terrain heights match vanilla dumps. |
| E2b | Dimensions, dimension types and biome sources. Heights and `min_y` from `dimension_type`; `multi_noise` parameter lists built into vanilla's `Climate.RTree`; a runtime biome registry so datapack biomes get ids beyond vanilla's (client sync already sends dynamic registries). | Vanilla's parameters build the same tree as `assets/multi_noise_biome_tree.json`. Terralith biome placement matches vanilla dumps (biome per quart at a fixed set of positions). |
| E2c | Surface rules (`material_rule`, `material_condition`). | Vanilla rules rebuilt at runtime equal codegen; Terralith surface blocks match dumps. |
| E2d | Carvers, configured and placed features per biome, from JSON at runtime. Feature types Pumpkin lacks are listed in this file with their users. | Vanilla's features rebuilt at runtime equal codegen; feature placement in Terralith and Incendium chunks matches dumps for every supported type. |
| E2e | Structures, structure sets, processors (template pools and templates are already dynamic). | Incendium and Nullscape structures start at the same positions as vanilla and their pieces match. |

## Out of scope

Bedrock worldgen. Datapacks shipped as zip files (Pumpkin loads directory packs only; unpacking
them is a pack-builder job, F3).
