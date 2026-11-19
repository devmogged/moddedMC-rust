# P1d: Create contraptions

Ported from Create 6.0.10 (`content/contraptions`, 185 files), method by method, with Create's
names. Follows Create stage 4 (depot, chute, funnels, processing).

## Shape

- A contraption is the set of blocks a controller moved, held as Create's `Contraption` data
  (`blocks`, `blockEntities`, actors, seats, storage, anchor, bounds) and saved as Create's NBT.
- Controlled contraptions (piston, pulley, gantry, bearings) keep that data in the controller's
  block entity in `mods/create`, saved with the controller's chunk, instead of in an entity. Create
  saves it on `ControlledContraptionEntity`; the NBT stays the same so a later entity port can
  load it. Minecart and train contraptions need a mod entity and come later.
- Blocks are drawn with `moddedmc:block_displays`: one display per block, all on the
  contraption's anchor, each block's offset in `translation` and the contraption's rotation in
  `rotation`, blended with `transform_interpolation` so rotation looks smooth between ticks.
  Content states show as their carrier on stock clients until C5.
- Server-side collision follows `ContraptionCollider.collideEntities` for non-player entities.
  Create skips players on the server (`PlayerType.SERVER`) because the Create client collides
  them. Stock clients don't, so players fall through moving contraptions; see stage P1d-6.

## Stages

Commit each stage separately with scenarios.

| Stage | Scope | Proof |
| --- | --- | --- |
| P1d-1 | Core: `Contraption.searchMovedStructure`, `BlockMovementChecks` (movement-necessary, attached, brittle, sticky faces), `StructureTransform`, `addBlocksToWorld` disassembly with rotation and mirroring of states, block entity data carried through, NBT. | Unit tests against Create's rotation of real states (stairs, logs, belts, shafts). |
| P1d-2 | Translating: mechanical piston (poles, head, sticky), gantry carriage and shaft, rope pulley and elevator pulley. `TranslatingContraption.isCollidingWithWorld` stalls. | Bot scenario: a piston pushes a 3x3 platform out and back; blocks land where Create puts them; a stalled piston stops at the obstacle. |
| P1d-3 | Rotating: mechanical bearing, windmill bearing (sails give stress capacity to the network, `WindmillBearingBlockEntity.getGeneratedSpeed`), clockwork bearing. Angles advance as `MechanicalBearingBlockEntity.tick`. | Scenario: a windmill with 8 sails powers a network at the RPM and capacity Create gives; a bearing turns a structure 90 degrees and disassembles it rotated. |
| P1d-4 | Super glue (entity and selection), linear and radial chassis, sticker. | Scenario: glued and chassis-attached blocks move together; unattached ones stay. |
| P1d-5 | Entities and actors: `collideEntities` for mobs and items, seats (`SeatEntity` riding), `MovementBehaviour` for drill, saw, harvester, plough, roller; mounted storage (`MountedStorageManager`) so chests move with their items. | Scenario: a drill on a piston breaks a block and drops it; a chest keeps its items across a move; a mob rides a seat. |
| P1d-6 | Players on stock clients. Measure two ways for translating contraptions: invisible no-AI shulkers following each top surface block, and teleport-based carrying of players standing on a contraption. Keep the one that holds a player on a moving platform without jitter at 20 and 50 ms latency; rotating contraptions stay without player collision until the companion does it (C5). | Bot with client physics stands on a moving piston platform and on an elevator and stays on top. |

## API

Uses what exists: block world reads and writes, block use, item stacks, containers, block
displays, entities. Ask for an ABI addition (in the report, not by editing `api/` or the host)
when a stage needs one, for example moving a passenger to a seat entity.
