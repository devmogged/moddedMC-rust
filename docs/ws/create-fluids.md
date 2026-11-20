# Create fluids: WS-2

Branch: `ws/create-fluids`. Scope: `mods/create/src/fluids/**`, basin and mixer. Server ports: 26020-26029.

## M0: source and interface study

Source: [Create 6.0.10](https://github.com/Creators-of-Create/Create/tree/mc1.21.1-6.0.10), tag `mc1.21.1-6.0.10`, commit `ac0c444d9828da3453ae8cc65338e8de063286fb`, Minecraft 1.21.1 NeoForge. Local source: `D:\mc-ref\create-6.0.10-source\src\main\java\com\simibubi\create\content\`. This source is MIT-licensed; its textures, sounds, models and language files are reserved and must remain outside this repository. The target runtime is Minecraft Java 26.3; note version-dependent differences during original-server comparisons.

### Class and call-chain map

| Java class (under `content/`) | Methods and behavior to preserve | Stage |
| --- | --- | --- |
| `fluids/FluidTransportBehaviour` | `initialize`, `tick`, `read/write`, `getProvidedOutwardFluid`, `addPressure`, `wipePressure`. Each face owns a connection. Three phases, in order: `WAIT_FOR_PUMPS`, `FLIP_FLOWS`, `IDLE`; detect conflicting inbound fluids before propagating them. | M1 |
| `fluids/PipeConnection` | `manageSource`, `determineSource`, `manageFlows`, `flipFlowsIfPressureReversed`, `tickFlowProgress`, `serializeNBT/deserializeNBT`. Per-face inbound/outbound pressures, source, flow, and ephemeral network. A completed inbound flow is the fluid offered to another pipe. | M1 |
| `fluids/FluidPropagator` | `propagateChangedPipe` and `resetAffectedFluidNetworks` traverse affected connected faces; `isOpenEnd` distinguishes handlers and blocked faces; `getPumpRange` comes from Create config. | M1 |
| `fluids/FluidNetwork` | `tick`, `reset`: traverse at most 16 propagation cycles/tick; obtain the source's sided fluid handler, simulate a drain and each target fill, distribute remainders, then execute. Fluid identity includes components; avoid mixing fluids. | M1 |
| `fluids/FlowSource` | `OtherPipe`, `FluidHandler`, `Blocked`, `provideFluid`, `provideHandler`: resolve each connection without loading a neighbor chunk. | M1 |
| `fluids/pipes/FluidPipeBlock` and `FluidPipeBlockEntity` | Six directional connection properties and `waterlogged`; update connections after placement, neighbor change and wrench. | M1 |
| `fluids/pipes/GlassFluidPipeBlock`, `EncasedPipeBlock`, `StraightPipeBlockEntity` | Straight-axis and alternate state for glass, encased six-face states, per-face flow persistence when wrenched. | M1 |
| `fluids/pump/PumpBlockEntity` | `onSpeedChanged`, `updatePressureChange`, `distributePressureTo`, `searchForEndpointRecursively`. Kinetic RPM produces directional pressure; branching divides pressure. Reverse pumping follows the block facing and rotation. | M1 |
| `fluids/OpenEndedPipe` | `removeFluidFromSpace`, `provideFluidToSpace` and its fluid handler. Source fluid is 1000 mB; removal of a waterlogged source schedules a fluid tick; the infinite-water case uses `getNewLiquid` preview. Never modify an unloaded world location. | M1 |
| `fluids/pipes/VanillaFluidTargets` | `drainBlock`: full water/lava cauldron yields 1000 mB; five honey levels yield 250 mB; empty and partial cauldrons do not drain. | M1 |
| `fluids/tank/FluidTankBlockEntity` | `updateConnectivity`, `applyFluidTankSize`, `removeController`, `read/write`. `ConnectivityHandler.formMulti` chooses controller; capacity is `width² * height * capacityMultiplier`; only the controller persists `TankContent`, `Size` and `Height`. | M2 |
| `processing/basin/BasinBlockEntity` | `addBehaviours`, `read/write`, `tick`, `acceptOutputs`, `getHeatLevelOf`. Input and output each have two 1000 mB tanks; output disallows insertion; item and fluid buffers and preferred outputs persist. | M3 |
| `kinetics/mixer/MechanicalMixerBlockEntity` | `tick`, `getMatchingRecipes`, `startProcessingBasin`, `continueWithPreviousRecipe`. 40-tick stroke, recipe execution at tick 20 after speed-dependent processing delay; preserve heat and speed gates. | M3 |
| `kinetics/press/MechanicalPressBlockEntity` | Existing press processing is integrator-owned; reuse it for basin compacting without rewriting its depot/belt behavior. | M3 |
| `fluids/spout/SpoutBlockEntity` | `onItemReceived`, `whenItemHeld`, `tick`, fluid NBT, recipe-based item filling on belts and depots. | M4 |
| `fluids/drain/ItemDrainBlockEntity` | `tryInsertingFromSide`, `tick`, `continueProcessing`, `setHeldItem`, held stack persistence and emptying logic. | M4 |
| `fluids/hosePulley/HosePulleyBlockEntity` | `onSpeedChanged`, `tick`, `getMovementSpeed`, offset and fluid handler persistence. | M5 |
| `fluids/pipes/SmartFluidPipeBlockEntity`, `valve/FluidValveBlockEntity` | Fluid filtering, directional valve throughput, and neighbor changes. | M5 |
| `fluids/tank/storage/FluidTankMountedStorage` | Mounted tank storage and portable fluid interface work after moving contraptions. | M5 |

### Host capability inventory

Current interface definitions: `docs/MOD-ABI.md`, `api/src/content.rs`, `api/src/containers.rs` at `8f1b600`. The latest `origin/ws/abi` branch did not exist at M0 inspection.

| Operation | Host availability / required action |
| --- | --- |
| Read/write custom and vanilla block states (loaded only), neighbor notifications, block entity NBT, content tick callback | Existing `moddedmc:block_world` and `moddedmc:content`. Use named state IDs and owned NBT; do not rely on host Rust structs. |
| Receive world positions and kinetic RPM | Existing Create `Kinetics::get`, `Be` and `Content::on_tick`. Share speed inside the mod without an engine hook. |
| Read/write `FluidState`, test source/waterlogging/replaceability, schedule fluid ticks, check ultra-warm dimension, source regeneration preview | Missing. `N:\mmc-coord\requests\WS-2-02.md` requests a generic `moddedmc:fluid_world` table. |
| Obtain a sided NeoForge-like `IFluidHandler` for block endpoints, simulate/execute fills and drains with component-preserving stacks | Missing. `N:\mmc-coord\requests\WS-2-01.md` requests `moddedmc:fluid_handlers`; provider registration for Create's own handlers needs WS-0 agreement. |
| Access vanilla item containers and serialized item stacks, play block sounds | Existing `moddedmc:containers`, `moddedmc:item_stacks`, `moddedmc:effects`; generic item-container fluid conversion must be ported from Create `GenericItemFilling` and `GenericItemEmptying`. |
| Analog comparator output for tank/basin states | WS-0 M3 plans generic analog-output registration and updates; do not emulate comparator behavior with a stand-in. |
| Register and consume mixing, compacting, filling and emptying recipes | The `moddedmc:resources` table and existing recipe parsing utilities are available; verify recipe type loading and item/fluid matching in M3/M4. |
| Render fluid levels/flows, item progress and particle data on a stock client | Asset pack can show models, but fluid-specific dynamic rendering is a companion-client concern. Gameplay remains server-side. |

### Source-backed invariants to test next

- `PipeConnection.tickFlowProgress`: `1/32 + clamp(pressure/128, 0, 1) * 31/32`; unpressurized completed flow cannot transfer unless the behavior retains flow and pressure requirements.
- `FluidNetwork.tick`: `CYCLES_PER_TICK = 16`, transfer speed is `max(1, (int)(inboundPressure/2))`, simulation precedes execution, and branch remainders follow the source's iteration order.
- `PipeConnection.serializeNBT`: directional nested NBT `Pressure`, `OpenEnd`, `Flow.Fluid`, `Flow.In`, and incomplete `Flow.Progress`.
- `FluidTransportBehaviour.tick`: pressure changes must reach every pipe before flow reversal, and every reversal must finish before the idle flow pass.
- `VanillaFluidTargets.drainBlock`: full water/lava cauldrons and five-level honey; use original Java samples as oracle.
- `FluidTankBlockEntity.read/write`: `Controller`, `Window`, `Size`, `Height`, `TankContent`, `Boiler`, and restart behavior.

### Verification and next milestone

M0 is a source and dependency audit; there are no player-visible changes in this milestone. Java source class and method names above were checked against commit `ac0c444`; no parity or benchmark claim is made. The ABI requests are external coordination files and remain pending WS-0 review.

M1 acceptance: place water-source and discharge locations with a powered pump and pipes, observe correct pressure direction, conservation and source placement/drain, reverse the pump and repeat, then save/restart. Compare the same topology and tick counts against Create 6.0.10 on NeoForge 1.21.1; record the cross-version differences. Keep pure connection/flow logic unit-tested before connecting it to a host. When WS-0 publishes shared binaries, run `scripts/scenario.ps1` on port 26020 with a dedicated WS-2 run directory. Measure a 64-pipe network on both servers under identical load before making performance claims.

M0 reviewer verdict: pending WS-R.

## M1: pipe transport implementation, first slice

Files: `mods/create/src/fluids/pipe_connection.rs`, `transport.rs`, and module wiring in `mods/create/src/lib.rs`. This code ports the deterministic part of Create's `PipeConnection` and `FluidTransportBehaviour`: signed inbound/outbound pressures, flow direction and progress, component-bearing fluid identity, one-sided pressure reversal, source selection, collision detection and the three-phase tick ordering. The source and endpoint adapters are deliberately absent until the generic fluid-handler and fluid-world ABI requests are answered. This module is not yet registered as content and changes no player-visible behavior.

Verified on the owner's Windows machine in this isolated worktree:
- `cargo fmt -p create`
- `cargo test -p create --lib`: 23 passed, 0 failed (nine new tests for pipe and transport flow rules; existing kinetic, belt, depot and funnel tests also pass)
- `cargo clippy -p create --all-targets -- -D warnings`: passed
- `git diff --check`: passed

These are Rust unit tests derived from Create 6.0.10 source rules, not an original Java runtime oracle. M1 remains open. Next source slices: FluidNetwork's simulation/execution and endpoint discovery, pump pressure BFS, block registrations, world persistence and the water-to-pool scenario. Complete original-server comparisons only after the shared host endpoints and WS-0 binaries exist. Do not mark M1 done on compilation alone.
