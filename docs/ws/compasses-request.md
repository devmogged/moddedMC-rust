# WS-14 to WS-0: versioned biome and structure world lookup for compass mods

## Java source and required semantics

Nature's Compass 26.3-3.3.1, `BiomeSearchWorker.java` (source commit `2202890ba7eec2312e6ea5c7734b2847e6b3a1c7`):

```java
biomeResolver = level.getChunkSource().getGenerator().getBiomeSource().createCachingResolver(level.getChunkSource().randomState());
final Biome biomeAtPos = biomeResolver.getNoiseBiome(sampleX, sampleY, sampleZ).value();
```

The mod scans quart-resolution noise biomes at selected Y values, including ungenerated chunks. A loaded-world `getBiome(BlockPos)` substitution changes results and performance. Explorer's Compass 26.3-3.3.2, `StructureSearchWorker.java` (source commit `3dcbd585fc100719fd6aac078941a818047395cc`):

```java
StructureCheckResult result = level.structureManager().checkStructurePresence(chunkPos, structure, placement, false);
ChunkAccess chunkAccess = level.getChunk(chunkPos.x(), chunkPos.z(), ChunkStatus.STRUCTURE_STARTS);
StructureStart structureStart = level.structureManager().getStartForStructure(structure, chunkAccess);
```

It also enumerates structure sets and placement strategies (concentric rings, random spread and generic), checks starts, and obtains the placement's locate position, not simply the chunk center. Both need dimension-specific dynamic registries, identifiers and tags.

## Proposed append-only host ABI (conceptual signatures)

Do not modify existing fields. Append an optional versioned group of synchronous, borrowed operations to `HostApi` or expose them through `get_interface("moddedmc:worldgen_lookup", 1)` following current ABI negotiation. A suitable C contract is:

```c
// 1 = found/present, 0 = absent, -1 = invalid dimension/id, -2 = worldgen data unavailable.
int32_t noise_biome_id(Str dimension, int32_t quart_x, int32_t quart_y, int32_t quart_z, Sink id_out);
// Return a stable registry snapshot as length-prefixed identifiers and tags through Sink.
int32_t registry_entries(Str dimension, uint32_t kind, Sink encoded_out);
// kind: 1 = biomes, 2 = structures, 3 = structure sets and their placement descriptors.
// A structure check must expose tri-state presence: present, absent, needs-structure-start-load.
int32_t structure_presence(Str dimension, Str structure_id, int32_t chunk_x, int32_t chunk_z, uint32_t placement_id, Pos* locate_pos);
```

The structure operation's `placement_id` must be stable within a returned registry snapshot and describe the original placement algorithm. If `needs-structure-start-load`, the host should provide a bounded deferred query that resolves actual structure starts at `ChunkStatus.STRUCTURE_STARTS` without blocking the tick. Alternatively expose a generic bounded `locate` operation that accepts an exclusion set and allows a continuation cursor, but verify that it preserves original ordering, radii, max-sample accounting and find-next semantics; a single nearest-structure function cannot transparently reproduce all workers.

All `Str` inputs are borrowed for the call, and `Sink` bytes are copied before return (existing ABI convention). No foreign pointer remains live beyond the call. The sink serializes UTF-8 identifier bytes with explicit length; invalid UTF-8, invalid dimensions, unloaded/unavailable worldgen and unknown biome/structure IDs return explicit errors rather than fabricated misses. Calls are server-side world-specific; do not expose mutable server object pointers. Tick-safe reads only; structure-start loads must not block executor threads.

## Verification contract

Run equal-seed, equal-dimension fixtures against both original NeoForge 26.3 mods and the Rust host: noise biome at quart positions across chunk boundaries and multiple Y values, random-spread and concentric-ring structure placement, negative/unknown IDs, unchanged generation and world reload, exact locate position, no chunk-force-generation on pure noise probes, and stable search sample counts/progress. Measure 10 concurrent compass searches on TPS and memory. Use 26.3 vanilla `/locate` as an additional cross-check, not a replacement for the mod oracle. Keep the host generic; the spiral, worker scheduling, exclusions, compass UI/state and configs belong in `mods/compasses`.
