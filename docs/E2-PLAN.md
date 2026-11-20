# E2: datapack worldgen

Terralith, Tectonic, Incendium and Nullscape ship as worldgen datapacks. Before E2, Pumpkin
generated terrain from built-in data only. Codegen in `tools/pumpkin-codegen` reads the vanilla
datapack in `assets/` and emits Rust (`noise_router.rs`, `noise_settings.rs`, `material_rule.rs`, `biome.rs`,
`configured_feature.rs`, `placed_feature.rs`, `carver.rs`, `structures.rs`). Datapack worldgen
files were already loaded by `dynamic_registry_loader.rs` for client registry sync.

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

## E2a implementation

World opening reads enabled directory packs in their saved order. Later packs replace earlier
entries with the same resource ID, including when a density function in one namespace references
a function or noise in another. A world with no terrain overrides keeps the generated settings
and component stacks. Generators keep the settings they opened with. Restart after enabling or
changing a worldgen pack.

The shared parser handles density functions, noise settings and normal-noise parameters.
Pumpkin uses the selected default block, fluid, sea level, terrain shape and spawn targets.
Surface rules, biome registries and features still use the existing engine paths until E2b–E2d.
Custom noise parameters currently require legacy normalization with the default base amplitude.
Settings that fail validation are logged and fall back to the generated dimension settings.

The Tectonic reference is [3.0.29-datapack for Java 26.3](https://modrinth.com/datapack/tectonic/version/pxgiJaJp).
It was the only standalone datapack release returned by Modrinth for 26.3. The archive was
verified against Modrinth's SHA-512 and used from scratch storage.

### E2a parity

- Runtime settings and every component stack matched the generated overworld, amplified, large
  biomes, Nether and End routers. The existing proto-chunk noise dump tests use the runtime
  builder for their overworld, Nether and End fixtures.
- Vanilla Java 26.3 and Pumpkin matched the first-air and ocean-floor heights in all 2,048
  Tectonic columns at seed 20261120, in chunks (0,0) through (1,1) and (64,64) through (65,65).
  In the vanilla run, turn off structures in `server.properties`, pass `-DMC_DEBUG_ENABLED` and the
  `MC_DEBUG_DISABLE_SURFACE`, `MC_DEBUG_DISABLE_CARVERS` and `MC_DEBUG_DISABLE_FEATURES`
  JVM flags, force-load those chunks and save. Read `WORLD_SURFACE` and `OCEAN_FLOOR` from
  the saved Anvil chunks. On Pumpkin, open the same seed with the same enabled pack, run the
  matching proto chunks through noise with no structure starts, and prime and snapshot heights.
- The ordinary server run also matched all 1,024 saved `WORLD_SURFACE` columns in the first
  four chunks. Use `[world.chunk.compression]` with `algorithm = "ZLib"` and `level = 6`
  to read both saves with the same Anvil decoder. Pumpkin's full Anvil chunk omitted
  `OCEAN_FLOOR`, which the noise-stage comparison reads directly from the proto chunk.

### E2a vanilla generation timings

Criterion `chunk_gen`, seed 42, 20 samples per group, pack profile on the same Windows host.
Times in milliseconds are mean estimates. The repeats ran before/after and after/before
on the same logical CPUs (affinity mask `0x54`).

| Run | Full chunk before | Full chunk after | Noise before | Noise after |
| --- | ---: | ---: | ---: | ---: |
| Initial, default affinity | 12.504 | 13.925 | 5.962 | 6.016 |
| Fixed CPU set, first pair | 13.861 | 14.082 | 4.765 | 4.618 |
| Fixed CPU set, second pair | 13.967 | 13.607 | 4.939 | 4.364 |

The initial full-chunk increase did not repeat consistently on the fixed CPU set.
