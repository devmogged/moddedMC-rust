# WS-14: Explorer's Compass and Nature's Compass

## M0 survey (2026-11-20)

### Exact upstream release and permission boundary

- Explorer's Compass: `26.3-3.3.2-neoforge`, NeoForge on Minecraft 26.3, 2026-10-04. Source checkout `D:\mc-ref\explorers-compass-latest` at `3dcbd585fc100719fd6aac078941a818047395cc`, `gradle.properties` confirms the version. Upstream: https://github.com/MattCzyr/ExplorersCompass; CurseForge project `491794`.
- Nature's Compass: `26.3-3.3.1-neoforge`, NeoForge on Minecraft 26.3, 2026-10-04. Source checkout `D:\mc-ref\natures-compass-latest` at `2202890ba7eec2312e6ea5c7734b2847e6b3a1c7`, `gradle.properties` confirms the version. Upstream: https://github.com/MattCzyr/NaturesCompass; CurseForge project `252848`.
- Each upstream repo has `LICENSE.md`: Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International, **not MIT/Apache**. Treat code, models, textures, translations and sounds as licensed material, and do not copy upstream resources into the permissively licensed workspace by default. Before shipping a derivative Rust implementation, confirm the owner's authorization/licensing plan for public commercial use and provide required attribution and share-alike terms. Sources can be read locally without adding them to git. The two source trees were independently checked; no version from the historical buja pack was used.

### Content and interfaces inventory

| Feature | Upstream Java/resource source | Required host function or current status |
| --- | --- | --- |
| One compass item per mod | `item/*CompassItem.java`, `registry/*Registry.java` | Custom items are supported (inspect item registry/ABI at M1), item use with player identity and stack-specific state needs assessment |
| Compass recipe and repair | `data/*/recipe/*compass.json`, two recipe/advancement files in Nature's; inspect Explorer's recipe files | Vanilla crafting recipe and recipe unlock path; import data only under compatible license |
| Vanilla compass item tag | `data/minecraft/tags/items/compasses.json` | Existing tag registration needs targeted check |
| Saved target, radius, sample count, visited results | `item/*CompassItem.java`, `util/CompassState.java`, component registrations in root `*Compass.java` | Stack-associated data component/state, including save/restart and client sync; verify item NBT/component ABI before M1 |
| Click item to open search screen | `item/*CompassItem.java`, `gui/*CompassScreen.java` | Server item-use callback and client-side NeoForge GUI; cannot assume stock client has mod GUI |
| Search action, find next, teleport, sync | `network/SearchPacket.java`, `SearchForNextPacket.java`, `TeleportPacket.java`, `SyncPacket.java` | Four custom payload types per mod. Stock 26.3 client cannot decode NeoForge mod screen/payload without companion; specify compatibility strategy |
| Selectable registry entries | `util/BiomeUtils.java` / `StructureUtils.java`, GUI search lists, filters and sorting classes | Need dimension-aware registry enumeration and biome/structure tags; **missing** in current ABI |
| Nature's biome spiral and distinct next result | `worker/BiomeSearchWorker.java`, `worker/WorldWorkerManager.java` | **Missing** deterministic noise-biome resolver (`createCachingResolver`, `getNoiseBiome`) at quart coordinates, with dimension and world generation state |
| Explorer's structure placement scanning | `worker/StructureSearchWorker.java`, `GenericSearchWorker.java`, `RandomSpreadSearchWorker.java`, `ConcentricRingsSearchWorker.java`, `SearchWorkerManager.java` | **Missing** structure-set placement and `checkStructurePresence`/structure-start access, including validated locate position; simple nearest-known-structure scan would not match |
| Per-mod settings | `config/ConfigHandler.java` | Server config load, per-player permissions and client preferences: preserve each default and range |
| Compass needle and HUD | `client/*Angle*.java`, `client/*Overlay.java`, `assets/*/items`, `models/item/*`, `textures/item/*` | Client-only 32-frame compass model and overlay. Stock client requires a resource pack for visuals and a distinct non-NeoForge UI path |
| Teleport and XP eligibility | `network/TeleportPacket.java`, `util/PlayerUtils.java`, sorting classes | Host teleport/permission/experience interfaces need exact ABI check; must authorize on server, not trust packet |
| Blocks, block entities, mobs, worldgen, commands | Both `src/main` trees | No mod-registered blocks, block entities, entities, worldgen features or commands found in the Java inventory; check all registration callbacks before marking exhaustive |

### Explicit search mechanics checked in Java

- Nature's `BiomeSearchWorker` initializes `sampleSpace = sampleSpaceModifier * biomeSize` (default modifier 16), `maxRadius = radiusModifier * biomeSize` (default modifier 2500), `maxSamples = 50000`. It uses origin-outward vertical samples at step 64, converts block coordinates to quart coordinates, resolves noise biomes (not just already-loaded biome blocks), walks a cardinal square spiral, and increments samples only after completing a candidate. Search bounds use `<=` for both radius and sample count. Every 500 radius units it reports floored rounded radius. Next-search distinguishes previous biome areas by line sampling and a gap threshold `clamp(samples/4,1,10)`; adjacent locations within two sample spaces are considered connected.
- Explorer's `StructureSearchWorker` initializes `maxRadius = 10000`, `maxSamples = 100000`, `maxNextSearches = 100`. Its worker continuation uses strict `<` for radius and samples. It scans structure placement algorithms, checks `StructureCheckResult`, and may load `ChunkStatus.STRUCTURE_STARTS` if presence is uncertain. It reports progress in 250 radius increments. Generic/rings/random-spread workers require separate method-by-method audit before implementing.
- Each `WorldWorkerManager.tick` budgets from a 50 ms tick and grants a minimum 10 ms window when lagged, dispatching `doWork()` repeatedly until budget exhausted. A Rust port should preserve externally observable progress and fairness without blocking game ticks.
- Both have search, find-next, teleport, and sync custom packets. Server handlers validate a compass is held, but the whole trust chain and server permissions need audit.
- Config defaults from Java: Nature's `maxNextSearches=25`, `allowTeleport=true`, `displayCoordinates=true`, `defaultXpLevels=0`, `compassDurability=0`; Explorer's `allowTeleport=true`, `displayCoordinates=true`, `defaultXpLevels=0`, `compassDurability=0`. Both define blacklist and per-target XP overrides, plus client overlay settings.

### Host gap and next checkpoints

The append-only `api/src/abi.rs` and `docs/MOD-ABI.md` do not expose world biome resolver or structure placement inspection. Request WS-0 capability `compass world lookup` once for both mods; do not approximate with chunk/block scans. The request file is `docs/ws/compasses-request.md` and must be filed through `coord.mjs request` rather than a normal GitHub issue.

M0 remains **partial** until both source trees are exhaustively categorized, the actual jar metadata and bundled licenses are checked, and the license/distribution boundary is accepted. The host request was filed as `devmogged/mmc-coord#72`.

### Initial native code slice (M1 partial)

`mods/compasses` now registers the two compass identifiers as native drop-in items, with an explicit CC BY-NC-SA 4.0 crate license and vanilla compass carriers (no upstream artwork). `biome_spiral.rs` implements Nature's `BiomeSearchWorker` cardinal spiral scheduling, inclusive radius/sample budget and floor-to-quart conversion. The `// TODO(abi): #72` marker isolates the missing worldgen lookup. **Not yet functional:** compass search, target NBT/data components, UI, recipe datapack, performance, and persistence. The stock-client visual fallback is not a full rendering parity claim.

Verified locally with `cargo fmt --package compasses`, `cargo test -p compasses` (3 passed), and `cargo clippy -p compasses --all-targets -- -D warnings` (passed). No server/bot scenario or original NeoForge oracle was run. An untracked `tests/scenarios/compasses-items.txt` existed concurrently and was not authored, modified or staged in this session; leave it for its owner to finish.

Next work: verify crate-item registration and pack load against the shared engine; add verified exact recipe data and an obtain/craft/use/restart scenario; then port remaining search and item state after ABI #72 is answered. Search mechanics and stock-client GUI are not yet implemented or claimed parity-complete.
