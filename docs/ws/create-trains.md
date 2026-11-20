# WS-3 Create trains

Branch: `ws/create-trains`. Source: Create 6.0.10 (Minecraft 1.21.1, NeoForge), upstream tag `mc1.21.1-6.0.10`, commit `ac0c444d9828da3453ae8cc65338e8de063286fb`. Local reference: `D:\mc-ref\create-trains-ws3\src\main\java\com\simibubi\create\content\trains\` (164 Java files). The original mod's code is MIT; do not commit its restricted assets. Target server: Java 26.3.

## M0: source and interface survey

| Java entry points and methods | Rust destination / dependencies |
| --- | --- |
| `track/TrackBlock.java` (`getConnected`, shape and axis handling), `ITrackBlock.walkConnectedTracks`, `TrackBlockItem.select/useOn` | `trains/track.rs`: straight, diagonal, sloped and junction blockstates, first/second-click item selection, waterlogging and block use |
| `track/TrackPlacement.tryConnect/placeTracks`, `TrackBlockEntity.addConnection/validateConnections/writeTurns/read` | `trains/track.rs`, `trains/bezier.rs`: segment rules, survival consumption, reciprocal curved connections, on-block-entity NBT |
| `track/BezierConnection.Runtime.determineHandles/computeLength/getSegmentT`, `secondary`, `incrementT` | `trains/bezier.rs`: Catnip Bezier geometry and 16-sample length, half-block segments, reversible curves |
| `graph/TrackNodeLocation`, `TrackNode`, `TrackEdge` (`getLength/canTravelTo`) | `trains/graph.rs`: half-block integer position with Y pixel offset and dimension, directed edges, turn geometry |
| `track/TrackPropagator.onRailAdded/onRailRemoved/isValidGraphNodeLocation` | `trains/graph.rs`: bounded frontier traversal, node spacing, merging/splitting graphs |
| `graph/TrackGraph` (`connectNodes/removeNode/transferAll/findDisconnectedGraphs/write/read`) | `trains/graph.rs`: node/edge ownership, points and signals, two directed edges per physical connection, NBT |
| `GlobalRailwayManager.levelLoaded/tick/tickTrains` and `RailwaySavedData.save/load` | `trains/railway.rs`: overworld-owned world-wide graphs and trains; railway NBT `RailGraphs`, `SignalBlocks`, `Trains`, `DimensionPalette` in saved key `create_tracks`; waiting trains tick before moving trains |
| `graph/EdgeData/EdgePointStorage`, `signal/SignalPropagator.walkSignals`, `SignalBoundary`, `SignalEdgeGroup` | `trains/signals.rs`: directed edge points, crossings, edge groups, reservations |
| `station/StationBlockEntity`, `GlobalStation`, `station/StationMarker`; `observer/TrackObserver` | `trains/stations.rs`: edge markers, redstone control, assembly and disassembly |
| `entity/Train`, `Carriage`, `CarriageContraption`, `CarriageContraptionEntity`, `TravellingPoint`, `Navigation` | `trains/carriage.rs`: bogeys, graph travel, schedules, collisions, loaded carriage entities and passengers; requires integrator P1d contraptions |
| `schedule/ScheduleRuntime.tick/tickConditions/read/write`, `Schedule`, schedule conditions and destinations | `trains/schedule.rs`: pre/in/post-transit progression, route selection, AND/OR wait conditions, per-train saved schedules |

### Host interfaces and ordering

Already available in MOD-ABI.md: `moddedmc:content` and `block_world` for custom blockstates, world read/write and block entity NBT; `moddedmc:interaction` for the track item's two clicks and stack custom-data; `moddedmc:entities` for loaded-entity view and movement; `moddedmc:block_displays` and `moddedmc:displays` for server-driven vanilla-carrier visuals; `moddedmc:resources` and `moddedmc:tags` for materials/recipes; `moddedmc:effects` for placement sounds.

Missing for M1: durable server/world-wide custom saved data, independent of loaded chunks, including load at startup and save at the normal server world-save barrier. Create's global railway data is not representable by a transient `World::id()` or any single block entity. Request WS-3-01 covers this. May also need load/reload notifications to reconstruct state before tracks tick.

Missing for M2/M3: generic graph marker interaction and menus for station assembly; exact needs will be checked against the applicable Java method before requesting. M3 needs non-mob entity registration/spawning, rider mounting/dismounting, position sync and carriage collision/contraption support. `moddedmc:mobs` registers only living mobs and is not a substitute for `CarriageContraptionEntity`. Ask WS-0 rather than adding any host logic here. Server-side motion and visuals must be checked with bots; client interpolation and stock-client passenger behavior need owner play tests.

Ordering dependencies: M0 and core M1 math are independent of contraptions. M1 world integration waits on WS-0 persistent-data capability and shared test binaries. M2 uses the graph/points from M1. M3 follows integrator P1d. M4 follows navigation in M3. Do not claim feature parity from pure-logic tests.

### Java behavior to preserve in M1

- `TrackNodeLocation(double x,y,z)`: X/Z are `floor(round(value * 2))` using **Java's round-to-positive-ties**, Y is `floor(y) * 2`. Y offset is `yOffsetPixels / 16`; dimensions participate in location equality.
- `TrackPropagator.isValidGraphNodeLocation`: junction and endpoint nodes, material changes, forced nodes, bend threshold `7/8`, then periodic 16-block nodes on straight tracks. Graph propagation has a 1,000-frontier-entry emergency limit.
- `TrackGraph.connectNodes` installs both directed edges, the reverse `BezierConnection.secondary()` for turns, and checks edge intersections. Removing rails can split a graph and detach affected trains.
- `TrackEdge.canTravelTo`: cross-dimensional edges always permit transitions; otherwise the directions' dot product must exceed `7/8`.
- `RailwaySavedData` writes `RailGraphs`, `SignalBlocks`, `Trains`, then the dimension palette. Its storage key is `create_tracks` in the overworld; loading graph IDs and signals must happen before trains.
- `TrackBlockItem` first click keeps `TRACK_CONNECTING_FROM` on the held item; second click validates geometry and material availability; sneaking clears selection.

## M0 evidence

Checked the source directly at tag `mc1.21.1-6.0.10` / commit `ac0c444`. This is a source audit, not a running Create server comparison. No server scenario is applicable to a documentation-only milestone. M1 must add differential geometry fixtures and player-visible scenarios before being called complete.

## Milestone log

- M0 complete: source survey and interface inventory committed and pushed as `2fcdfaf` (`docs(create-trains): map railway sources and host gaps`) on 2026-11-20. `git diff --check` clean; no runtime tests for the documentation-only milestone. `N:\mmc-coord\requests\WS-3-01.md` asks WS-0 for persistent per-save global data.
- M1 partial, graph and curve kernel: `mods/create/src/trains/graph.rs` reproduces half-block position rounding, adjacent chunk probes, graph-node reductions, directed straight edges, connect/removal and connected components. `mods/create/src/trains/bezier.rs` models Create's handle calculation, 16-step length approximation, reverse curve, step LUT and track costs. Both modules are **test-only** and not registered into live world behavior until the rest of M1 exists. Original Java integration tests, straight/diagonal/curve placement, graph save/reload, branch/signal edges and network synchronization are still open.
- M1 unit evidence: 12 new Rust tests (seven graph and five curve), 26 Create tests in total, passed on 2026-11-20; `cargo clippy -p create --all-targets -- -D warnings`, `cargo fmt -p create -- --check`, and `git diff --check` passed. These are source-derived fixtures, not execution against original Java mod.
