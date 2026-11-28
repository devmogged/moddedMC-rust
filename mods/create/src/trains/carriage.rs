//! Carriage capture, wheel geometry and disassembly at Train Stations.
use std::collections::{HashSet, VecDeque};

use moddedmc_api::{
    Pos,
    content::{BlockUpdate, Content, World},
    stacks::ItemDisplay,
};
use serde::{Deserialize, Serialize};

use super::{
    bogey::{AssemblyTrack, is_bogey},
    controls,
    graph::TrackGraph,
    math::Vec3,
    railway,
    save::RailwayData,
    schedule::Schedule,
    targeting::{Selection, coords, directed_edge, pos},
    travelling_point::{TravellingPoint, select_edge},
};
use crate::{
    DIRECTIONS, Kinetics,
    contraption::{
        BlockMovement, BlockMovementChecks, Blocks, ContraptionDisplays, StructureTransform,
    },
    display_model::ModelDisplay,
    kinetics::{FACES, face_axis},
    offset,
};

// The station state and every captured block must fit one block-world write.
const MAX_TRAIN_BLOCKS: usize = 4095;

#[derive(Deserialize, Serialize)]
pub(super) struct Train {
    pub(super) station: Selection,
    #[serde(default)]
    pub(super) id: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) stopped_at: Option<Selection>,
    pub(super) carriages: Vec<Carriage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) schedule: Option<Schedule>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Carriage {
    anchor: [i32; 3],
    pub(super) blocks: Blocks,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) wheels: Vec<[TravellingPoint; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame: Option<CarriageFrame>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
struct CarriageFrame {
    center: [f64; 3],
    yaw: f32,
}

impl Train {
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
        let network = railway::read(&world).map_err(|_| "Could not read the rail network")?;
        let mut wheel_points = Vec::with_capacity(bogeys.len());
        for &(_, bogey) in &bogeys {
            wheel_points.push(
                wheel_pair(&network, world, content, &station, bogey, track.step)
                    .ok_or("Extend the straight track beyond the bogeys")?,
            );
        }
        let direction = FACES
            .iter()
            .position(|&face| face == track.step)
            .ok_or("The train needs straight, level track")? as u8;
        let mut occupied: HashSet<_> = trains
            .iter()
            .filter(|train| train.station.dimension == station.dimension)
            .flat_map(|train| train.carriages.iter())
            .flat_map(|carriage| {
                let (transform, _) = carriage.transform();
                carriage
                    .blocks
                    .block_list
                    .iter()
                    .map(move |block| transform.apply(block.local_pos()))
            })
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
            let next = index + 1 + usize::from(second_bogey);
            carriages.push(Carriage {
                anchor: coords(anchor),
                blocks,
                wheels: wheel_points[index..next].to_vec(),
                frame: None,
            });
            index = next;
        }
        if !forward_controls {
            return Err("Add Train Controls facing the assembly direction");
        }
        Ok(Self {
            station,
            id: 0,
            stopped_at: None,
            carriages,
            schedule: None,
        })
    }

    pub(super) fn count(&self) -> usize {
        self.carriages.len()
    }

    pub(super) fn wheel_count(&self) -> usize {
        self.carriages
            .iter()
            .map(|carriage| carriage.wheels.len() * 2)
            .sum()
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

    pub(super) fn cab(&self) -> Option<usize> {
        self.carriages.iter().position(|carriage| {
            carriage
                .blocks
                .palette
                .iter()
                .any(|entry| entry.name == controls::ID)
        })
    }

    pub(super) fn copy_position(&mut self, source: &Self) {
        self.stopped_at.clone_from(&source.stopped_at);
        self.schedule.clone_from(&source.schedule);
        for (carriage, moved) in self.carriages.iter_mut().zip(&source.carriages) {
            carriage.wheels.clone_from(&moved.wheels);
            carriage.frame = moved.frame;
        }
    }

    pub(super) fn dock(&self) -> Option<&Selection> {
        if self.carriages.first()?.frame.is_some() {
            self.stopped_at.as_ref()
        } else {
            Some(&self.station)
        }
    }

    pub(super) fn graph<'a>(&self, network: &'a RailwayData) -> Option<&'a TrackGraph> {
        let wheel = self.carriages.first()?.wheels.first()?.first()?;
        network
            .graphs
            .iter()
            .find_map(|entry| wheel.edge(&entry.graph).map(|_| &entry.graph))
    }

    pub(super) fn end_wheel(&self, forward: bool) -> Option<&TravellingPoint> {
        if forward {
            self.carriages.first()?.wheels.first()?.first()
        } else {
            self.carriages.last()?.wheels.last()?.last()
        }
    }

    pub(super) fn recover_wheels(
        &mut self,
        network: &RailwayData,
        world: World<'_>,
        content: Content,
    ) -> Option<()> {
        if self.wheel_count() != 0 {
            return Some(());
        }
        let (edge, _) = directed_edge(network, world, content, &self.station)?;
        let direction = edge.direction(true);
        let step = [-direction.x.round() as i32, 0, -direction.z.round() as i32];
        let wheels = self
            .carriages
            .iter()
            .map(|carriage| {
                let mut bogeys: Vec<_> = carriage
                    .blocks
                    .block_list
                    .iter()
                    .filter(|block| is_bogey(&carriage.blocks.palette[block.palette_index()].name))
                    .map(|block| block.local_pos())
                    .collect();
                bogeys.sort_by_key(|local| local[0] * step[0] + local[2] * step[2]);
                bogeys
                    .into_iter()
                    .map(|local| {
                        wheel_pair(
                            network,
                            world,
                            content,
                            &self.station,
                            offset(pos(carriage.anchor), local),
                            step,
                        )
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()?;
        for (carriage, points) in self.carriages.iter_mut().zip(wheels) {
            carriage.wheels = points;
        }
        Some(())
    }

    /// Move the leading wheel first, then follow its route with the rest of the train.
    pub(super) fn advance<'a>(
        &mut self,
        world: Option<World<'_>>,
        graph: &'a TrackGraph,
        distance: f64,
        steer: f64,
        select: impl Fn(&'a super::graph::TrackEdge, bool) -> Option<&'a super::graph::TrackEdge>,
    ) -> Option<f64> {
        let mut wheels: Vec<_> = self
            .carriages
            .iter()
            .flat_map(|carriage| carriage.wheels.iter().flatten().cloned())
            .collect();
        let mut follow = None;
        let mut travelled = distance;
        let forward = distance >= 0.0;
        if !forward {
            wheels.reverse();
        }
        for wheel in &mut wheels {
            let moved = wheel.travel(graph, travelled, |edge, forward| {
                if follow.is_some() {
                    select_edge(graph, edge, forward, steer, follow)
                } else {
                    select(edge, forward)
                }
            })?;
            if follow.is_some() && (moved - travelled).abs() > 1.0e-6 {
                return None;
            }
            travelled = moved;
            follow = wheel.edge(graph);
        }
        if travelled == 0.0 {
            return Some(0.0);
        }
        if !forward {
            wheels.reverse();
        }
        let mut points = wheels.as_chunks::<2>().0.iter();
        let frames = self
            .carriages
            .iter()
            .map(|carriage| {
                let wheels = points
                    .by_ref()
                    .take(carriage.wheels.len())
                    .cloned()
                    .collect::<Vec<_>>();
                let frame = carriage.wheel_frame(graph, &wheels)?;
                let [x, y, z] = frame.center.map(|v| v.floor() as i32);
                if let Some(world) = world {
                    super::targeting::state_at(world, Pos { x, y, z })?;
                }
                Some((wheels, frame))
            })
            .collect::<Option<Vec<_>>>()?;
        for (carriage, (wheels, frame)) in self.carriages.iter_mut().zip(frames) {
            carriage.wheels = wheels;
            carriage.frame = Some(frame);
        }
        self.stopped_at = None;
        Some(travelled)
    }

    pub(super) fn disassemble(&self, world: World<'_>, checks: &BlockMovementChecks) -> bool {
        let Some(first) = self.carriages.first() else {
            return false;
        };
        let (transform, translation) = first.transform();
        let aligned = |angle: f32, shift: [f32; 3]| {
            (angle / 90.0 - (angle / 90.0).round()).abs() < 0.001
                && shift.into_iter().all(|v| v.abs() < 0.01)
        };
        if !aligned(transform.angle, translation) {
            return false;
        }
        let inverse = StructureTransform {
            angle: -transform.angle,
            ..transform
        };
        let mut blocks = first.blocks.clone();
        let mut occupied: HashSet<_> = blocks.positions(transform.anchor).collect();
        for carriage in &self.carriages[1..] {
            let (frame, shift) = carriage.transform();
            if !aligned(frame.angle, shift) || (frame.angle - transform.angle).abs() > 0.01 {
                return false;
            }
            let anchor = inverse.apply([
                frame.anchor.x - transform.anchor.x,
                frame.anchor.y - transform.anchor.y,
                frame.anchor.z - transform.anchor.z,
            ]);
            if carriage
                .blocks
                .positions(anchor)
                .any(|pos| !occupied.insert(pos))
            {
                return false;
            }
            blocks.append_at(transform.anchor, anchor, &carriage.blocks);
        }
        // Place every carriage in one batch, so an obstruction leaves the whole train docked.
        blocks.place_structure(world, checks, transform, None)
    }
}

fn wheel_pair(
    network: &RailwayData,
    world: World<'_>,
    content: Content,
    station: &Selection,
    bogey: Pos,
    step: [i32; 3],
) -> Option<[TravellingPoint; 2]> {
    let rail = offset(bogey, [0, -1, 0]);
    let selection = Selection {
        target: coords(rail),
        ..station.clone()
    };
    let (edge, distance) = directed_edge(network, world, content, &selection)?;
    let graph = &network
        .graphs
        .iter()
        .find(|saved| saved.graph.edge(&edge.from, &edge.to).is_some())?
        .graph;
    let mut points = [
        TravellingPoint::new(edge, distance),
        TravellingPoint::new(edge, distance),
    ];
    // StandardBogeyBlock wheels are one block to either side of the bogey center.
    for (point, (travel, sign)) in points.iter_mut().zip([(1.0, -1), (-1.0, 1)]) {
        let expected = offset(rail, step.map(|axis| sign * axis));
        let location = Vec3 {
            x: f64::from(expected.x) + 0.5,
            y: f64::from(expected.y),
            z: f64::from(expected.z) + 0.5,
        };
        if point.travel_straight(graph, travel) != Some(travel)
            || point.location(graph)?.distance(location) > 1.0e-6
        {
            return None;
        }
    }
    Some(points)
}

impl Carriage {
    fn wheel_frame(
        &self,
        graph: &TrackGraph,
        wheels: &[[TravellingPoint; 2]],
    ) -> Option<CarriageFrame> {
        let pose = |wheels: &[[TravellingPoint; 2]]| {
            let [front, rear] = wheels.first()?;
            let (front, rear) = (front.location(graph)?, rear.location(graph)?);
            let center = front.plus(rear).scale(0.5);
            let direction = if let Some([front, rear]) = wheels.get(1) {
                center.minus(
                    front
                        .location(graph)?
                        .plus(rear.location(graph)?)
                        .scale(0.5),
                )
            } else {
                front.minus(rear)
            };
            Some((
                center,
                (-direction.z).atan2(direction.x).to_degrees() as f32,
            ))
        };
        let (_, previous_yaw) = pose(&self.wheels)?;
        let (center, next_yaw) = pose(wheels)?;
        let yaw = next_yaw - previous_yaw + self.frame.map_or(0.0, |frame| frame.yaw);
        Some(CarriageFrame {
            center: [center.x, center.y + 1.5, center.z],
            yaw: (yaw + 180.0).rem_euclid(360.0) - 180.0,
        })
    }

    pub(super) fn transform(&self) -> (StructureTransform, [f32; 3]) {
        let (anchor, angle, translation) = if let Some(frame) = self.frame {
            let [x, y, z] = frame.center.map(|coord| coord.floor() as i32);
            let anchor = Pos { x, y, z };
            let translation = std::array::from_fn(|i| {
                (frame.center[i] - f64::from(coords(anchor)[i]) - 0.5) as f32
            });
            (anchor, frame.yaw, translation)
        } else {
            (pos(self.anchor), 0.0, [0.0; 3])
        };
        (
            StructureTransform {
                anchor,
                axis: 1,
                angle,
            },
            translation,
        )
    }

    pub(super) fn driver_position(&self) -> [f64; 3] {
        let (transform, shift) = self.transform();
        let local = self
            .blocks
            .block_list
            .iter()
            .find_map(|block| {
                (self.blocks.palette[block.palette_index()].name == controls::ID)
                    .then(|| block.local_pos())
            })
            .unwrap_or([0, 1, 0]);
        let (sin, cos) = f64::from(transform.angle).to_radians().sin_cos();
        let [x, y, z] = local.map(f64::from);
        let offset = [x * cos + z * sin, y - 7.0 / 16.0, z * cos - x * sin];
        let anchor = coords(transform.anchor);
        std::array::from_fn(|i| f64::from(anchor[i]) + 0.5 + f64::from(shift[i]) + offset[i])
    }
}

#[derive(Default)]
pub(super) struct CarriageDisplays {
    blocks: ContraptionDisplays,
    bogeys: Option<Vec<BogeyDisplay>>,
}

struct BogeyDisplay {
    local: ItemDisplay,
    model: &'static str,
    shown: ModelDisplay,
}

impl CarriageDisplays {
    pub(super) fn apply_clicks(&mut self, world: World<'_>, carriage: &mut Carriage) -> bool {
        self.blocks.apply_clicks(world, &mut carriage.blocks)
    }

    pub(super) fn show(&mut self, world: World<'_>, carriage: &Carriage) {
        let (frame, translation) = carriage.transform();
        self.blocks
            .show_moving(world, &carriage.blocks, frame, translation);
        let bogeys = self.bogeys.get_or_insert_with(|| {
            carriage
                .blocks
                .block_list
                .iter()
                .filter_map(|block| {
                    let entry = carriage.blocks.palette.get(block.palette_index())?;
                    let model = match entry.name.as_str() {
                        "create:small_bogey" => "create:moddedmc/parts/bogey/small",
                        "create:large_bogey" => "create:moddedmc/parts/bogey/large",
                        _ => return None,
                    };
                    let rotation = if entry.properties.get("axis").is_some_and(|axis| axis == "x") {
                        let half = std::f32::consts::FRAC_1_SQRT_2;
                        [0.0, half, 0.0, half]
                    } else {
                        [0.0, 0.0, 0.0, 1.0]
                    };
                    Some(BogeyDisplay {
                        local: ItemDisplay {
                            position: block.local_pos().map(|axis| f64::from(axis) + 0.5),
                            rotation,
                            scale: 1.0,
                            context: 0,
                            interpolation: 1,
                        },
                        model,
                        shown: ModelDisplay::default(),
                    })
                })
                .collect()
        });
        for bogey in bogeys {
            let transform = frame.item_display(bogey.local, translation);
            bogey.shown.show(world, bogey.model, &transform);
        }
    }

    pub(super) fn clear(&mut self, world: World<'_>) {
        self.blocks.clear(world);
        if let Some(bogeys) = &mut self.bogeys {
            for bogey in bogeys {
                bogey.shown.clear(world);
            }
        }
    }
}
