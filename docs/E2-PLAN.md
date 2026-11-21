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

## After E2e

**Full Java height ranges.** Widen generation Y coordinates, size heightmaps from each
dimension, and extend the random-tick section mask. Preserve existing saves and vanilla
packet bytes. Compare tall and entirely negative worlds against Java 26.3 and verify
that both Anvil and Pump can save and reopen them.

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
E2d adds Java 26.3's enabled, disabled and legacy noise normalization modes for runtime noise overrides.
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

### Datapack dimensions

Startup resolves each level key and its dimension type independently. It loads named
dimensions from enabled packs and saved world settings, preserves their separate world
folders, and gives Java spawn packets the type's synced index. Vanilla level keys and
their existing types keep their normal packet fields. Changing dimension definitions
requires a restart.

The bot fixture used two flat levels sharing one type with minimum Y -80 and height 384.
Both generated bedrock at Y -80. The respawn packets carried holder 4 and the distinct
level keys `fixture:deep` and `fixture:copy`. A block placed in `fixture:deep` stayed
there across restart. The bot rejoined `fixture:copy` at Y -78 with holder 4 in Login.

The current generation and storage types support a bottom Y of -128 to 112 in multiples
of 16 and heights of 256 to 496 in multiples of 16. The loader reports unsupported
ranges before generation. Full Java height ranges still need signed generation
coordinates, variable-width heightmaps and a larger random-tick section mask.

## E2c surface rules

Levels compile the selected noise settings' material rule once at startup. References
resolve across enabled packs and the built-in registries, with the highest pack taking
priority. The generator reuses its existing evaluator for the compiled rule. It preserves
block state properties, resolves biome names and tags to synced IDs, and samples 3D noise
thresholds at the block's Y. Worlds without overrides use the generated rules.

The runtime compiler rebuilt all seven generated vanilla rule trees exactly. The
Terralith 2.6.5+26.3 run at seed 20261120 matched all **786,432 block states** from Y -64
through 319 in chunks (0,0) through (1,1) and (64,64) through (65,65). Vanilla 26.3
generated and saved those chunks with structures, carvers and features disabled.
Pumpkin ran the same pack through the surface stage and wrote the staged chunks to
Anvil. The harness compared names and properties across both saved regions.

The back-to-back vanilla `chunk_gen` benchmark used seed 42, the pack profile,
20 samples and CPU mask `0x54`. Mean full-chunk time was 12.105 ms before and
12.164 ms after. Surface time was 1.467 ms before and 1.451 ms after. Neither
difference was statistically significant. The final evaluator keeps the original
rule layout and caches repeated noise samples at the same position.

`moddedmc-harness compare-blocks <reference-region-dir> <subject-region-dir> <x0> <y0> <z0> <x1> <y1> <z1>`
compares an inclusive block area. It prints up to twenty coordinates and counts every
state pair that differs. Compare only corresponding generation stages and dimensions.

The supported ore-vein rules still use the built-in copper and iron density pipeline.
Custom ore-vein density functions are reported as unsupported. Relative anchors above the
bottom or below the top currently use the existing signed-byte offsets.

## E2d carvers

Enabled packs can add or replace named carvers. Each biome uses its declared carvers in
file order. Codegen and the runtime loader share the same carver and height-provider
parser. Pack precedence applies even if an otherwise unmodified vanilla biome references
the replaced carver. Unresolved or unsupported carvers are reported at startup.

Incendium required Java's default normal-noise normalization. A fixture checked three
coordinates and the maximum amplitude for each normalization mode against the Java 26.3
codec, using identical seeds. All sampled values matched bit-for-bit. The runtime carver
compiler also rebuilt all four generated vanilla carver configurations.

| Reference pack | Compared Y | Block states | Differences |
| --- | ---: | ---: | ---: |
| [Terralith 2.6.5+26.3](https://modrinth.com/datapack/terralith/version/fqnsIp6d) | -64 to 319 | 786,432 | 0 |
| [Incendium 5.5.4+26.3 alpha](https://modrinth.com/datapack/incendium/version/1phtTdhj) | 0 to 255 | 524,288 | 0 |

Both archives matched Modrinth's SHA-512. Incendium was the latest standalone datapack
published for Java 26.3 when tested. Seed 20261120, chunks (0,0) through (1,1) and
(64,64) through (65,65), in the overworld and Nether respectively. On Java, disable
structures and disable feature generation with the debug switch. Force-load the chunks and
save. On Pumpkin, run through carvers, write the staged chunks to Anvil, and compare
every state with the harness. Configured and placed features are the next E2d slice.

The pack-profile stock `chunk_gen` benchmark used seed 42, 20 samples and CPU mask
`0x54`. The figures are Criterion slope estimates from two runs in opposite orders.

| Binary order | Full chunk before / after | Carvers before / after | Features before / after |
| --- | ---: | ---: | ---: |
| Before, then after | 10.386 / 10.620 ms | 782.73 / 690.73 µs | 1.117 / 1.047 ms |
| After, then before | 10.482 / 10.343 ms | 707.68 / 662.56 µs | 1.020 / 1.060 ms |

The full-chunk result reversed direction with run order. The carver point estimate
was lower in both runs.

### E2d runtime feature wiring

Biomes retain their placed-feature IDs and step order from the winning pack. Noise
generators using datapack biome lists build the decoration order once from the source
biome order. A skipped feature with a registered ID still takes its original slot,
so later decoration seeds keep their indexes. Packs with a feature-order cycle report
the feature involved and skip decoration in that level. Restart after changing packs.

The first slice decodes no-op, simple block, fill layer, block pile, single-block
replacement, netherrack replacement blobs, ore and scattered ore configurations.
It shares the entry codec with codegen and the height-provider sampler with carvers.
Other configurations are reported and skipped. Built-in feature definitions remain
available without a datapack.

| Pack | Named feature files | Files using a decoded type | Largest pending families |
| --- | ---: | ---: | --- |
| Terralith | 508 | 128 | Trees (189), random selectors (66), vegetation patches (36), disks (25) |
| Incendium | 33 | 11 | Vegetation patches (10), random selectors (4), trees (4) |

These counts classify types. Named provider resolution and feature behavior still
need to be checked against both packs. Terralith also uses the shelf-mushroom tree
decorator, which Pumpkin does not implement. The current loader only accepts named
entries in biome feature lists. Nested placed holders and datapack block tags still
need resolution.

A live Java 26.3 fixture put a gold marker in two positions and then replaced the
first with diamond in the same biome step. Pumpkin generated both expected states
after enabling the pack and restarting. The harness found zero differences across
the 256 blocks at Y 300 in chunk (64,64), seed 20261120.

The first full-decoration sample used the same seed and compared chunks (64,64)
through (65,65) with each named datapack enabled. Terralith differed at 289 of 393,216
positions, including coal ore, moss, hanging moss and bubble columns. Incendium differed
at 2,737 of 262,144 positions, mostly netherrack, red sand, red terracotta and vines.
Java ran with structures disabled. Pumpkin used its default built-in structure sets.
These runs record the earliest live baseline for feature loading. The E2d parity
check still needs identical structure settings and the remaining feature codecs.

The pack-profile stock benchmark compared this slice with E2c at seed 42, with 20
samples and CPU mask `0x54`. Criterion slope estimates were 10.715 and 10.701 ms
for full chunks, and 1.104 and 1.122 ms for decoration. Neither change was
statistically significant.

### E2d tree configurations and block tags

Runtime tree entries use the existing trunk, foliage, root and decorator implementations.
Rule-based state providers try their conditions in file order. Named providers resolve
across the enabled packs. At startup, Pumpkin compiles block tags into the feature
definitions using the existing merged membership resolver, including `replace: true`.
Missing tree types or decorators are logged with their feature ID.

The runtime compiler rebuilt the extracted vanilla oak into the same generated block
states as codegen. A Java 26.3 and Pumpkin fixture grew an oak over a named provider
whose custom block tag selected a gold foundation. At seed 20261120, the frozen fixture
matched all 2,816 states at Y 294 through 304 in chunk (64,64). The running-world fixture
differed only at 34 leaf distances. Java schedules a leaf update from the edge of the
generated tree. Pumpkin's proto chunk originally dropped those block ticks.

The stock pack-profile benchmark against E2c used seed 42, CPU mask `0x54` and 20
samples. Criterion slope estimates were 11.262 and 10.955 ms for full chunks and
1.228 and 1.140 ms for decoration. This pair showed no slowdown.

Block and fluid ticks now survive proto-chunk promotion and relighting. A loaded chunk
with queued ticks enters the world's active scheduler, and `simple_block` honors its
`schedule_tick` field. In the tree fixture, the number of block differences after four
steps dropped from 34 to 12. After eight steps, all 2,816 states matched. The four-tick
leaf-distance timing gap remains open.

The stock pack-profile benchmark against the tree slice used seed 42, CPU mask
`0x54` and 20 samples. Criterion slope estimates were 10.725 and 10.688 ms for full
chunks and 1.110 and 1.091 ms for decoration. Neither difference was significant.

### E2d nested feature holders

The loader resolves named and inline configured and placed holders across enabled
packs. A placement shares its decoded children with other callers. Cycles and
unavailable references skip the affected entry. A named block state provider
falls back to the extracted vanilla definition when no enabled pack overrides it.

Runtime decoding now covers random, random boolean, weighted random and simple
random selectors, sequences, disks, vegetation patches and waterlogged vegetation
patches. The Java 26.3 fixture used a named sequence to place the gold foundation
and a named random selector to grow the oak with the custom foundation tag. The
vanilla oak unit test exercises the built-in soil provider fallback separately.
At seed 20261120, the frozen Java and Pumpkin worlds matched all 2,816
states in chunk (64,64) from Y 294 through 304. The cycle test rejects a loop
between a named configured and a named placed entry and still resolves an independent
sequence with an inline child.

This check covers the sequence and random-selector path. The remaining selector,
disk and vegetation behavior still needs its own parity checks with real pack data.
Pack-profile clippy passed. The world suite passed 325 unit tests plus one save
test. The shared model passed five tests. The Nether biome hash test failed once,
then passed in isolation and in five complete reruns.

Stock benchmarks used seed 42 and CPU mask `0x54`. Runs varied with machine
load. Full chunk slopes in milliseconds were 11.242 to 11.881, 13.794 to 15.523
with the new binary first, and 14.252 to 14.178 on a longer repeat. Decoration
slopes were 1.249 to 1.208, 2.159 to 1.712, and 1.928 to 1.561. The results do not
establish a consistent slowdown.

### E2d block columns and bamboo

The runtime loader and codegen share the column and bamboo input validation.
Codegen's output changed only in redundant integer suffixes. A column test
proved that truncation keeps the tip when the next layer is obstructed, and a
live fixture exercised a named tip provider and bamboo growth on grass. The
frozen Java 26.3 and Pumpkin worlds matched all 6,400 states in chunk (64,64)
from Y 294 through 318 at seed 20261120.

Fmt and pack-profile clippy passed across the touched crates. The world suite
passed 326 unit tests and one save test, and the shared model passed five tests.
The stock benchmark used seed 42, CPU mask `0x54` and 25 samples. Full chunk
slopes were 10.707 and 10.680 ms. Decoration slopes were 1.103 and 1.040 ms.
Criterion detected no full-chunk change and classified the decoration change
as within its noise threshold.

### E2d placement rules

The Java block-match rule checks a state's block before drawing its probability.
The per-layer count placement samples its provider at every loop condition,
including the terminating check. Focused tests reproduced both discrepancies
before the fix and passed after it.

The Java 26.3 fixture used a block match on an X-axis oak log and a uniform
count of one through four for gold markers. At seed 20261120 in chunk (64,64),
the original engine left the log in place and produced two extra gold markers.
After the fix, both servers replaced the log at (1028,290,1028) and placed one
gold marker at (1036,291,1033). The frozen snapshots matched all 512 states.
Fmt, clippy, 328 world unit tests and one save test passed.

The stock benchmark used seed 42, CPU mask `0x54` and 25 samples. In two
paired runs, full chunk slopes were 11.143 to 10.747 ms and 12.281 to 11.069 ms.
Decoration slopes were 1.087 to 1.148 ms and 1.149 to 1.168 ms. The first
decoration comparison detected a regression. The reverse-order repeat did
not detect a change. Host timings varied between pairs.