//! Manual control and stock-client carriage displays.
use std::{collections::HashMap, sync::OnceLock};

use moddedmc_api::{
    InterfaceUnavailable, Pos,
    abi::{
        PLAYER_INPUT_BACKWARD, PLAYER_INPUT_FORWARD, PLAYER_INPUT_JUMP, PLAYER_INPUT_LEFT,
        PLAYER_INPUT_RIGHT,
    },
    content::{self, Content, ContentEvent, World},
    entities::Entity,
    mobs::{self, Mobs, MobsExtension},
    stacks::ItemDisplay,
};

use super::{
    carriage::Train,
    graph::TrackGraph,
    railway,
    save::RailwayData,
    station,
    targeting::{Selection, directed_edge, state_at},
    travelling_point::select_edge,
};
use crate::{DISPLAYS, ENTITIES, contraption::ContraptionDisplays, offset};

// CTrains defaults, converted from metres per second (and per second squared) to blocks per tick.
const ACCELERATION: f64 = 3.0 / 400.0;
const TOP_SPEED: f64 = 28.0 / 20.0 * 0.75;
const TURN_SPEED: f64 = 14.0 / 20.0;
const ARRIVAL_EPSILON: f64 = 1.0e-6;
static INPUT: OnceLock<Mobs> = OnceLock::new();

pub(super) fn require() -> Result<(), InterfaceUnavailable> {
    let mobs = mobs::require()?;
    mobs.require_extension(MobsExtension::Taming)?;
    let _ = INPUT.set(mobs);
    Ok(())
}

#[derive(Default)]
pub(crate) struct TrainRuntime {
    trains: Option<Vec<Train>>,
    shown: HashMap<i32, Vec<ContraptionDisplays>>,
    drivers: HashMap<i32, Driver>,
    network: Option<(u64, RailwayData)>,
}

struct Driver {
    player: i32,
    seat: i32,
    carriage: usize,
    speed: f64,
    forward: bool,
}

impl Driver {
    fn keys(&self, world: World<'_>) -> Option<i32> {
        let input = *INPUT.get()?;
        ENTITIES.get()?.with_entity(world, self.player, |player| {
            (player.vehicle() == Some(self.seat) && input.game_mode(player) != Some(3))
                .then(|| input.player_input(player))
                .flatten()
        })?
    }

    fn accelerate(&mut self, target: f64) {
        let countering = self.speed * target < 0.0;
        let step = ACCELERATION * if countering { 2.0 } else { 1.0 };
        self.speed += (target - self.speed).clamp(-step, step);
    }

    fn stop_driving(&mut self, world: World<'_>) {
        if self.seat == 0 {
            return;
        }
        if let Some(entities) = ENTITIES.get() {
            entities.with_entity(world, self.player, |player| {
                if player.vehicle() == Some(self.seat) {
                    player.stop_riding();
                }
            });
        }
        if let Some(displays) = DISPLAYS.get() {
            displays.remove(world, self.seat);
        }
        self.seat = 0;
    }
}

fn seat(position: [f64; 3]) -> ItemDisplay {
    ItemDisplay {
        position,
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: 0.0,
        context: 0,
        interpolation: 1,
    }
}

impl TrainRuntime {
    pub(super) fn refresh(&mut self, world: World<'_>) {
        self.trains = None;
        if let Some(data) = station::read(world) {
            self.shown.retain(|id, shown| {
                let exists = data.trains.iter().any(|train| train.id == *id);
                if !exists {
                    for display in shown {
                        display.clear(world);
                    }
                }
                exists
            });
        }
    }

    pub(super) fn has_driver(&self, world: World<'_>, id: i32) -> bool {
        self.drivers
            .get(&id)
            .is_some_and(|driver| driver.keys(world).is_some())
    }

    pub(super) fn board_at_rail(
        &mut self,
        world: World<'_>,
        position: Pos,
        player: Entity<'_>,
    ) -> bool {
        let (Some(dimension), Some(data)) = (world.dimension_id(), station::read(world)) else {
            return false;
        };
        data.trains
            .iter()
            .find(|train| {
                train.station.dimension == dimension
                    && train.cab().is_some_and(|index| {
                        let (frame, _) = train.carriages[index].transform();
                        offset(frame.anchor, [0, -1, 0]) == position
                    })
            })
            .is_some_and(|train| self.board(world, train, player))
    }

    pub(super) fn board(&mut self, world: World<'_>, train: &Train, player: Entity<'_>) -> bool {
        if self
            .drivers
            .get(&train.id)
            .is_some_and(|driver| driver.speed != 0.0 || driver.keys(world).is_some())
        {
            return false;
        }
        let (Some(displays), Some(carriage)) = (DISPLAYS.get(), train.cab()) else {
            return false;
        };
        let position = train.carriages[carriage].driver_position();
        // The empty-scale display is a mount with no saved entity or visible item.
        let nbt = fastnbt::nbt!({ "id": "minecraft:stone", "count": 1i32 });
        let Ok(stack) = fastnbt::to_bytes(&nbt) else {
            return false;
        };
        let Some(seat_id) = displays.spawn_item(world, &stack, &seat(position)) else {
            return false;
        };
        if !player.start_riding(seat_id, false) {
            displays.remove(world, seat_id);
            return false;
        }
        let mut driver = Driver {
            player: player.id(),
            seat: seat_id,
            carriage,
            speed: 0.0,
            forward: true,
        };
        if let Some(mut old) = self.drivers.remove(&train.id) {
            old.stop_driving(world);
            driver.speed = old.speed;
            driver.forward = old.forward;
        }
        self.drivers.insert(train.id, driver);
        true
    }

    pub(super) fn tick(
        &mut self,
        world: World<'_>,
        content: Content,
        time: u64,
        events: &[ContentEvent],
    ) {
        let reload = self.trains.is_none();
        if reload {
            let (Some(dimension), Some(data)) = (world.dimension_id(), station::read(world)) else {
                return;
            };
            self.trains = Some(
                data.trains
                    .into_iter()
                    .filter(|train| train.station.dimension == dimension)
                    .collect(),
            );
        }
        let Some(trains) = self.trains.as_mut() else {
            return;
        };
        let mut moved = Vec::new();
        if !self.drivers.is_empty() {
            let revision = railway::revision();
            if self
                .network
                .as_ref()
                .is_none_or(|(old, _)| *old != revision)
            {
                self.network = railway::read(&world).ok().map(|data| (revision, data));
            }
            let mut stations = None;
            self.drivers.retain(|id, driver| {
                let Some(train) = trains.iter_mut().find(|train| train.id == *id) else {
                    driver.stop_driving(world);
                    return false;
                };
                let keys = driver.keys(world);
                if keys.is_none() {
                    driver.stop_driving(world);
                }
                if let Some((_, network)) = &self.network
                    && tick_driver(
                        world,
                        content,
                        network,
                        train,
                        driver,
                        keys.unwrap_or(0),
                        &mut stations,
                    )
                {
                    moved.push(*id);
                }
                driver.seat != 0 || driver.speed != 0.0
            });
        }
        if !moved.is_empty()
            && !station::save_positions(
                world,
                trains.iter().filter(|train| moved.contains(&train.id)),
            )
        {
            moddedmc_api::warn("Could not save train wheel positions");
            self.trains = None;
            for driver in self.drivers.values_mut() {
                driver.speed = 0.0;
                driver.stop_driving(world);
            }
            self.drivers.clear();
            return;
        }
        let refresh = reload
            || time.is_multiple_of(20)
            || events
                .iter()
                .any(|event| matches!(event.reason, content::LOADED | content::UNLOADED));
        for train in trains {
            let moving = moved.contains(&train.id);
            if refresh || moving {
                let shown = self.shown.entry(train.id).or_default();
                shown.resize_with(train.carriages.len(), ContraptionDisplays::default);
                for (carriage, display) in train.carriages.iter().zip(shown) {
                    let (frame, translation) = carriage.transform();
                    if state_at(world, offset(frame.anchor, [0, -1, 0])).is_some() {
                        display.show_moving(world, &carriage.blocks, frame, translation);
                    } else {
                        display.clear(world);
                    }
                }
            }
            if moving
                && let Some(driver) = self.drivers.get(&train.id)
                && driver.seat != 0
                && let Some(displays) = DISPLAYS.get()
                && let Some(carriage) = train.carriages.get(driver.carriage)
            {
                displays.update(world, driver.seat, &seat(carriage.driver_position()));
            }
        }
    }
}

fn tick_driver(
    world: World<'_>,
    content: Content,
    network: &RailwayData,
    train: &mut Train,
    driver: &mut Driver,
    keys: i32,
    stations: &mut Option<Vec<Selection>>,
) -> bool {
    let throttle = f64::from(
        i32::from(keys & PLAYER_INPUT_FORWARD != 0) - i32::from(keys & PLAYER_INPUT_BACKWARD != 0),
    );
    let jump = keys & PLAYER_INPUT_JUMP != 0;
    if driver.speed == 0.0 && (jump || throttle == 0.0) && train.dock().is_some() {
        return false;
    }
    if !jump && throttle == 0.0 && driver.speed == 0.0 {
        return false;
    }
    if train.recover_wheels(network, world, content).is_none() {
        driver.speed = 0.0;
        return false;
    }
    let Some(graph) = train.graph(network) else {
        driver.speed = 0.0;
        return false;
    };
    let steer = f64::from(
        i32::from(keys & PLAYER_INPUT_RIGHT != 0) - i32::from(keys & PLAYER_INPUT_LEFT != 0),
    );
    let forward = if driver.speed != 0.0 {
        driver.speed > 0.0
    } else if throttle != 0.0 {
        throttle > 0.0
    } else {
        driver.forward
    };
    let on_turn = [train.end_wheel(true), train.end_wheel(false)]
        .into_iter()
        .flatten()
        .any(|point| point.edge(graph).is_some_and(|edge| edge.turn.is_some()));
    let mut top_speed = if on_turn { TURN_SPEED } else { TOP_SPEED };
    if throttle < 0.0 {
        top_speed *= 0.25;
    }
    let braking_distance = driver.speed.powi(2) / (2.0 * ACCELERATION);
    let stop = if jump {
        let stops =
            stations.get_or_insert_with(|| station::stops(world, train.station.dimension.as_str()));
        approaching_station(
            world,
            content,
            network,
            graph,
            train,
            stops,
            forward,
            steer,
            (1.5 * braking_distance).max(32.0),
        )
    } else {
        None
    };
    let target = if jump {
        if stop
            .as_ref()
            .is_some_and(|(_, distance)| *distance > braking_distance)
        {
            if forward { top_speed } else { -top_speed }
        } else {
            0.0
        }
    } else {
        throttle * top_speed
    };
    driver.accelerate(target);
    let mut distance = driver.speed;
    if let Some((_, to_stop)) = &stop
        && (distance.abs() >= *to_stop || (distance == 0.0 && *to_stop <= ACCELERATION))
    {
        distance = if forward { *to_stop } else { -*to_stop };
    }
    if distance == 0.0 {
        return false;
    }
    let Some(moved) = train.advance(world, graph, distance, steer) else {
        driver.speed = 0.0;
        return false;
    };
    if moved.abs() > ARRIVAL_EPSILON {
        driver.forward = moved > 0.0;
    }
    if let Some((station, to_stop)) = stop
        && (moved.abs() - to_stop).abs() < ARRIVAL_EPSILON
    {
        train.stopped_at = Some(station);
        driver.speed = 0.0;
        if let (Some(entities), Some(&input)) = (ENTITIES.get(), INPUT.get()) {
            entities.with_entity(world, driver.player, |player| {
                input.message(player, "Train arrived at the station", true)
            });
        }
        return true;
    }
    if (moved - distance).abs() > ARRIVAL_EPSILON {
        driver.speed = 0.0;
    }
    moved.abs() > ARRIVAL_EPSILON
}

#[allow(clippy::too_many_arguments)]
fn approaching_station(
    world: World<'_>,
    content: Content,
    network: &RailwayData,
    graph: &TrackGraph,
    train: &Train,
    stops: &[Selection],
    forward: bool,
    steer: f64,
    max_distance: f64,
) -> Option<(Selection, f64)> {
    let wheel = train.end_wheel(forward)?;
    let mut edge = wheel.edge(graph)?;
    let mut start = wheel.offset();
    let mut travelled = 0.0;
    let stops: Vec<_> = stops
        .iter()
        .filter_map(|station| {
            let (target, offset) = directed_edge(network, world, content, station)?;
            if forward {
                Some((station, target, offset))
            } else {
                Some((
                    station,
                    graph.edge(&target.to, &target.from)?,
                    target.length() - offset,
                ))
            }
        })
        .collect();
    // shortcut: look ahead along the path selected by the driver's steering. Scheduled trains use routed destinations.
    while travelled <= max_distance {
        let next = stops
            .iter()
            .filter_map(|&(station, target, offset)| {
                if !std::ptr::eq(edge, target) {
                    return None;
                }
                let distance = (offset - start) * if forward { 1.0 } else { -1.0 };
                (distance >= ARRIVAL_EPSILON && travelled + distance <= max_distance)
                    .then_some((station, travelled + distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((station, distance)) = next {
            return Some((station.clone(), distance));
        }
        travelled += if forward {
            edge.length() - start
        } else {
            start
        };
        edge = select_edge(graph, edge, forward, steer, None)?;
        start = if forward { 0.0 } else { edge.length() };
    }
    None
}
