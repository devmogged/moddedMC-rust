//! Stationary carriage capture and disassembly at a Train Station.
use std::collections::{HashSet, VecDeque};

use moddedmc_api::content::{self, BlockUpdate, Content, ContentEvent, World};
use serde::{Deserialize, Serialize};

use super::{
    bogey::{AssemblyTrack, is_bogey},
    controls, railway,
    targeting::{Selection, coords, directed_edge, pos, state_at},
};
use crate::{
    DIRECTIONS, Kinetics,
    contraption::{
        BlockMovement, BlockMovementChecks, Blocks, ContraptionDisplays, StructureTransform,
    },
    kinetics::{FACES, face_axis},
    offset,
};

// The station state and every captured block must fit one block-world write.
const MAX_TRAIN_BLOCKS: usize = 4095;

#[derive(Deserialize, Serialize)]
pub(super) struct DockedTrain {
    pub station: Selection,
    carriages: Vec<Carriage>,
}

#[derive(Deserialize, Serialize)]
struct Carriage {
    anchor: [i32; 3],
    blocks: Blocks,
}

impl DockedTrain {
    pub(super) fn capture(
        world: World<'_>,
        content: Content,
        station: Selection,
        track: AssemblyTrack,
        checks: &BlockMovementChecks,
        kinetics: &Kinetics,
        trains: &[Self],
    ) -> Result<Self, &'static str> {
        let bogeys = track.bogeys(world, content);
        if bogeys.first().map(|&(i, _)| i) != Some(0) {
            return Err("Put the first bogey one rail past the station");
        }
        if bogeys.windows(2).any(|pair| pair[1].0 - pair[0].0 < 3) {
            return Err("Leave at least three blocks between bogeys");
        }
        let graph = railway::read(&world).map_err(|_| "Could not read the rail network")?;
        for &(_, bogey) in &bogeys {
            let rail = offset(bogey, [0, -1, 0]);
            // The StandardBogeyBlock wheel points are one block ahead and behind its center.
            for step in [track.step, track.step.map(|axis| -axis)] {
                let mut wheel = station.clone();
                wheel.target = coords(offset(rail, step));
                if directed_edge(&graph, world, content, &wheel).is_none() {
                    return Err("Extend the straight track beyond the bogeys");
                }
            }
        }
        let direction = FACES
            .iter()
            .position(|&face| face == track.step)
            .ok_or("The train needs straight, level track")? as u8;
        let mut occupied: HashSet<_> = trains
            .iter()
            .filter(|train| train.station.dimension == station.dimension)
            .flat_map(|train| train.carriages.iter())
            .flat_map(|carriage| carriage.blocks.positions(pos(carriage.anchor)))
            .collect();
        let mut carriages = Vec::new();
        let mut forward_controls = false;
        let mut block_count = 0;
        let mut index = 0;
        while index < bogeys.len() {
            let anchor = bogeys[index].1;
            let Some(BlockMovement::Movable(entry)) = checks.classify(world, anchor) else {
                return Err("The bogey must be loaded");
            };
            let (mut blocks, _) = checks
                .collect_attached(
                    world,
                    anchor,
                    VecDeque::from([(anchor, entry)]),
                    HashSet::new(),
                    None,
                )
                .ok_or("Could not move the blocks attached to the bogey")?;
            if blocks.block_list.len() <= 1 {
                return Err("Attach blocks to every bogey");
            }
            block_count += blocks.block_list.len();
            if block_count > MAX_TRAIN_BLOCKS {
                return Err("The train is too large");
            }
            if blocks
                .positions(anchor)
                .any(|position| !occupied.insert(position))
            {
                return Err("Carriages cannot overlap");
            }
            let mut second_bogey = false;
            for block in &blocks.block_list {
                let entry = &blocks.palette[block.palette_index()];
                let local = block.local_pos();
                if local != [0; 3] && is_bogey(&entry.name) {
                    if second_bogey {
                        return Err("A carriage can have at most two bogeys");
                    }
                    if bogeys.get(index + 1).map(|&(_, pos)| pos) != Some(offset(anchor, local)) {
                        return Err("Connect bogeys in order along the track");
                    }
                    second_bogey = true;
                }
                if entry.name == controls::ID {
                    let Some(facing) = DIRECTIONS.iter().position(|&facing| {
                        entry
                            .properties
                            .get("facing")
                            .is_some_and(|name| name == facing)
                    }) else {
                        return Err("The Train Controls have an invalid facing");
                    };
                    if face_axis(facing as u8) != face_axis(direction) {
                        return Err("Face Train Controls along the track");
                    }
                    forward_controls |= facing == usize::from(direction);
                }
            }
            blocks
                .capture_entities(world, anchor, kinetics)
                .ok_or("Could not capture the carriage's block data")?;
            for entry in &mut blocks.palette {
                if entry.name == controls::ID {
                    entry.properties.insert("open".into(), "true".into());
                }
            }
            carriages.push(Carriage {
                anchor: coords(anchor),
                blocks,
            });
            index += 1 + usize::from(second_bogey);
        }
        if !forward_controls {
            return Err("Add Train Controls facing the assembly direction");
        }
        Ok(Self { station, carriages })
    }

    pub(super) fn count(&self) -> usize {
        self.carriages.len()
    }

    pub(super) fn removals(&self, checks: &BlockMovementChecks) -> Option<Vec<BlockUpdate>> {
        let mut removals = Vec::new();
        for carriage in &self.carriages {
            removals.extend(carriage.blocks.removals(pos(carriage.anchor), checks)?);
        }
        Some(removals)
    }

    pub(super) fn detach_glue(&mut self, world: World<'_>) {
        for carriage in &mut self.carriages {
            carriage.blocks.detach_glue(world);
        }
    }

    pub(super) fn disassemble(&self, world: World<'_>, checks: &BlockMovementChecks) -> bool {
        let Some(first) = self.carriages.first() else {
            return false;
        };
        let transform = first.transform();
        let mut blocks = first.blocks.clone();
        for carriage in &self.carriages[1..] {
            blocks.append_at(transform.anchor, pos(carriage.anchor), &carriage.blocks);
        }
        // Place every carriage in one batch, so an obstruction leaves the whole train docked.
        blocks.place_structure(world, checks, transform, None)
    }
}

impl Carriage {
    fn transform(&self) -> StructureTransform {
        StructureTransform {
            anchor: pos(self.anchor),
            axis: 1,
            angle: 0.0,
        }
    }
}

#[derive(Default)]
pub(crate) struct TrainDisplays {
    carriages: Option<Vec<(Carriage, Option<ContraptionDisplays>)>>,
}

impl TrainDisplays {
    pub(super) fn refresh(&mut self, world: World<'_>) {
        if let Some(carriages) = self.carriages.take() {
            for (_, shown) in carriages {
                if let Some(mut shown) = shown {
                    shown.clear(world);
                }
            }
        }
    }

    pub(super) fn tick(
        &mut self,
        world: World<'_>,
        time: u64,
        events: &[ContentEvent],
        load: impl FnOnce() -> Option<Vec<DockedTrain>>,
    ) {
        if self.carriages.is_none() {
            let (Some(dimension), Some(trains)) = (world.dimension_id(), load()) else {
                return;
            };
            self.carriages = Some(
                trains
                    .into_iter()
                    .filter(|train| train.station.dimension == dimension)
                    .flat_map(|train| train.carriages.into_iter().map(|carriage| (carriage, None)))
                    .collect(),
            );
        } else if !time.is_multiple_of(20)
            && !events
                .iter()
                .any(|event| matches!(event.reason, content::LOADED | content::UNLOADED))
        {
            return;
        }
        let Some(carriages) = self.carriages.as_mut() else {
            return;
        };
        for (carriage, shown) in carriages {
            let frame = carriage.transform();
            let rail = offset(frame.anchor, [0, -1, 0]);
            if events
                .iter()
                .any(|event| event.position == rail && event.reason == content::UNLOADED)
                && let Some(mut display) = shown.take()
            {
                display.clear(world);
            }
            let loaded = state_at(world, rail).is_some();
            match (loaded, shown.is_some()) {
                (true, false) => {
                    let mut display = ContraptionDisplays::default();
                    display.show(world, &carriage.blocks, frame, [0.0; 3]);
                    *shown = Some(display);
                }
                (false, true) => {
                    if let Some(mut display) = shown.take() {
                        display.clear(world);
                    }
                }
                _ => {}
            }
        }
    }
}
