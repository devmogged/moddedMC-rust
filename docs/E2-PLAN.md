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

## E2b climate sources

The runtime climate tree uses the Java 26.3 fanout and stable ordering. The test rebuilds
the overworld and Nether trees and compares every node, bound and leaf against the
extracted vanilla trees. A separate check with the ordered parameter points emitted by
Java's preset providers matched both trees (7,594 overworld leaves and five Nether leaves).
The runtime search keeps the previous leaf per thread and per tree, so it preserves
vanilla's choice when two biome entries are equally close.

Enabled packs can replace built-in dimension generator definitions. Direct multi-noise
lists and checkerboards with explicit biome IDs work with the existing biome registry.
Saved generator settings preserve those source definitions through a round trip.
Dimension type overrides, new dimensions and synced runtime biome IDs remain in E2b.

The Terralith reference is [2.6.5+26.3](https://modrinth.com/datapack/terralith/version/fqnsIp6d).
The archive matched Modrinth's SHA-512. It adds 95 biome IDs and replaces the overworld
source with 1,708 climate entries. The full comparison needs the runtime registry.

The live smoke test used seed 20261120, an overworld override with one climate entry for
`minecraft:desert`, and a fresh chunk at (64,64). Both servers reported desert at
(1024,-32,1024), (1024,64,1024) and (1024,204,1024). Pumpkin reported the same after saving
and restarting. The 312 world tests and three shared-model tests passed. Regenerating
`biome.rs` after moving the biome metadata parser produced no generated source diff.

On the pack-profile `chunk_gen` benchmark at seed 42 (20 samples, CPU mask `0x54`), the
back-to-back baseline and climate-source binaries measured 11.254 and 10.843 ms for a full
chunk, 233.17 and 222.75 µs for biomes, and 3.226 and 3.294 ms for noise. The noise
difference was inconclusive (p = 0.13). An earlier run of the tree-only change showed
the opposite direction for full chunk and biome timings.

### Java spawn info

Java 26.3 writes the raw index in the synced `dimension_type` registry as a VarInt,
then the level resource key. Login and respawn share the encoder for these fields.
The serializer fixture uses holder index 130 and level key `fixture:deep`.

A stock-protocol bot captured the same payload bytes before and after the change for
overworld login (110 bytes), death and respawn in the overworld (65 bytes), and transfer
through a real Nether portal (66 bytes). The captured holder/key pairs were
`0 / minecraft:overworld`, `0 / minecraft:overworld` and `3 / minecraft:the_nether`.

### Runtime biome registry and Terralith parity

Runtime biome IDs follow the synced registry order. Vanilla IDs retain their existing
indexes, and the Java chunk palette uses the resulting registry size for direct encoding.
Existing world IDs remain valid across `/reload`. Restart to apply biome registry changes.
Both Anvil and Pump palettes save custom biome names so IDs can be resolved at the next
startup. The engine currently accepts up to 256 biome entries.

Terralith includes splines with points out of order. Java 26.3 accepts these in file order
through the datapack codec and searches the array as supplied. Pumpkin now does the same.
Before this fix, Pumpkin fell back to the built-in noise router and the harness counted
6,716 differences in the sampled quarts.

With the fix, the harness found zero differences across 12,288 saved Terralith biome quarts
at seed 20261120. The samples covered chunks (0,0) through (1,1) and (64,64) through
(65,65), each from Y -64 through 319. The vanilla run disabled surface, carver and feature
generation. Converting the subject's world from Anvil to Pump and back left all sampled
biomes unchanged. Surface rules, carvers and placed features are covered by the later stages.

The ordered-spline benchmark reran seed 42 on the same CPU mask with both binaries.
Full chunk generation measured 11.722 ms and 11.406 ms, biome generation 241.96 µs
and 216.87 µs, and noise 3.462 ms and 3.420 ms. Criterion found a change in biome
generation. The paired full-chunk and noise differences were inconclusive.
