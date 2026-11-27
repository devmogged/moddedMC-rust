//! Train Schedule items and the station/delay subset of Create's ScheduleRuntime.
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
};

use moddedmc_api::{
    RegistrationError, Registry,
    content::{Content, World},
    items::Item,
};
use serde::{Deserialize, Serialize};

use super::{
    carriage::Train,
    driving::{ACCELERATION, SIGNAL_CLEARANCE, TOP_SPEED, TURN_SPEED},
    graph::{TrackEdge, TrackGraph, TrackNodeLocation},
    math::Vec3,
    railway,
    save::RailwayData,
    signal::SignalPlan,
    station,
    targeting::{Selection, directed_edge, locate, pos, state_at},
    travelling_point::{TravellingPoint, next_edges},
};

pub(crate) const ID: &str = "create:schedule";
const DEFAULT_WAIT: u16 = 5 * 20;
const RETRY_TICKS: u64 = 20;
const EPSILON: f64 = 1.0e-6;

pub(super) fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    crate::ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: ID,
                display_name: "Train Schedule",
                carrier: "minecraft:paper",
                model: "create:item/schedule",
            },
        )
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Schedule {
    stops: [Selection; 2],
    next: usize,
    waiting: u16,
    cyclic: bool,
    finished: bool,
}

impl Schedule {
    pub(super) fn new(
        world: World<'_>,
        content: Content,
        train: &mut Train,
        start: Selection,
        destination: Selection,
    ) -> Option<Self> {
        let network = railway::read(&world).ok()?;
        train.recover_wheels(&network, world, content)?;
        let graph = train.graph(&network)?;
        let (target, offset) = station_point(world, content, &network, &destination)?;
        Route::new(graph, train.end_wheel(true)?, target, offset)?;
        let cyclic =
            station_point(world, content, &network, &start).is_some_and(|(start, start_offset)| {
                Route::new(
                    graph,
                    &TravellingPoint::new(target, offset),
                    start,
                    start_offset,
                )
                .is_some()
            });
        Some(Self {
            stops: [start, destination],
            next: 1,
            waiting: DEFAULT_WAIT,
            cyclic,
            finished: false,
        })
    }

    pub(super) fn destination(&self) -> Option<&Selection> {
        (!self.finished)
            .then(|| self.stops.get(self.next))
            .flatten()
    }

    pub(super) fn second_stop(&self) -> &Selection {
        &self.stops[1]
    }

    pub(super) fn status(&self) -> &'static str {
        if self.finished {
            "Schedule complete"
        } else if self.cyclic {
            "Cyclic schedule"
        } else {
            "One-way schedule"
        }
    }

    fn arrive(&mut self) {
        if self.cyclic {
            self.next ^= 1;
            self.waiting = DEFAULT_WAIT;
        } else {
            self.finished = true;
        }
    }
}

fn station_point<'a>(
    world: World<'_>,
    content: Content,
    network: &'a RailwayData,
    selection: &Selection,
) -> Option<(&'a TrackEdge, f64)> {
    if state_at(world, pos(selection.target)).is_some() {
        directed_edge(network, world, content, selection)
    } else {
        // A train can continue along the saved graph when nobody is near its destination.
        [
            Vec3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        ]
        .into_iter()
        .find_map(|axis| locate(network, selection, axis))
    }
}

type EdgeKey = (TrackNodeLocation, TrackNodeLocation);
type BorrowedKey<'a> = (&'a TrackNodeLocation, &'a TrackNodeLocation);

struct Route {
    path: Vec<EdgeKey>,
    index: usize,
    destination: f64,
}

impl Route {
    fn new<'a>(
        graph: &'a TrackGraph,
        start: &TravellingPoint,
        destination: &'a TrackEdge,
        distance: f64,
    ) -> Option<Self> {
        let start_edge = start.edge(graph)?;
        let initial = (&start_edge.from, &start_edge.to);
        let target = (&destination.from, &destination.to);
        let mut path = vec![initial];
        if initial != target || start.offset() > distance {
            type Prior<'a> = (f64, Option<BorrowedKey<'a>>);
            let mut frontier = BinaryHeap::new();
            let mut costs: HashMap<BorrowedKey<'a>, Prior<'a>> = HashMap::new();
            let enqueue =
                |edge: BorrowedKey<'a>,
                 cost: f64,
                 prior: Option<BorrowedKey<'a>>,
                 frontier: &mut BinaryHeap<_>,
                 costs: &mut HashMap<BorrowedKey<'a>, Prior<'a>>| {
                    if costs.get(&edge).is_none_or(|&(old, _)| cost < old) {
                        costs.insert(edge, (cost, prior));
                        // Nonnegative finite doubles have the same order as their bits.
                        frontier.push(Reverse((cost.to_bits(), edge)));
                    }
                };
            for edge in next_edges(graph, start_edge, true) {
                enqueue(
                    (&edge.from, &edge.to),
                    start_edge.length() - start.offset(),
                    None,
                    &mut frontier,
                    &mut costs,
                );
            }
            while let Some(Reverse((cost, key))) = frontier.pop() {
                let cost = f64::from_bits(cost);
                if key == target {
                    break;
                }
                if cost > costs.get(&key)?.0 {
                    continue;
                }
                let edge = graph.edge(key.0, key.1)?;
                for next in next_edges(graph, edge, true) {
                    enqueue(
                        (&next.from, &next.to),
                        cost + edge.length(),
                        Some(key),
                        &mut frontier,
                        &mut costs,
                    );
                }
            }
            let mut prior = target;
            let mut tail = Vec::new();
            loop {
                tail.push(prior);
                let Some(key) = costs.get(&prior)?.1 else {
                    break;
                };
                prior = key;
            }
            path.extend(tail.into_iter().rev());
        }
        Some(Self {
            path: path
                .into_iter()
                .map(|(from, to)| (from.clone(), to.clone()))
                .collect(),
            index: 0,
            destination: distance,
        })
    }

    fn leading_edge<'a>(
        &mut self,
        graph: &'a TrackGraph,
        wheel: &TravellingPoint,
    ) -> Option<&'a TrackEdge> {
        let edge = wheel.edge(graph)?;
        self.index += self.path[self.index..]
            .iter()
            .position(|(from, to)| edge.from == *from && edge.to == *to)?;
        Some(edge)
    }

    fn next<'a>(&self, graph: &'a TrackGraph, current: &TrackEdge) -> Option<&'a TrackEdge> {
        let index = self.path[self.index..]
            .iter()
            .position(|(from, to)| current.from == *from && current.to == *to)?;
        let (from, to) = self.path.get(self.index + index + 1)?;
        graph.edge(from, to)
    }

    /// Remaining distance and distances to the first red boundary and curve.
    fn distances(
        &self,
        graph: &TrackGraph,
        lead: &TrackEdge,
        offset: f64,
        red: &[(&TrackEdge, f64)],
    ) -> Option<(f64, f64, f64)> {
        let mut travelled = 0.0;
        let mut limit = f64::INFINITY;
        let mut next_turn = f64::INFINITY;
        let mut start = offset;
        for (index, (from, to)) in self.path[self.index..].iter().enumerate() {
            let edge = if index == 0 {
                lead
            } else {
                graph.edge(from, to)?
            };
            let end = if index + self.index + 1 == self.path.len() {
                self.destination
            } else {
                edge.length()
            };
            if edge.turn.is_some() {
                next_turn = next_turn.min(travelled);
            }
            for &(red_edge, red_offset) in red {
                if std::ptr::eq(edge, red_edge) && (start..=end).contains(&red_offset) {
                    limit = limit.min((travelled + red_offset - start - SIGNAL_CLEARANCE).max(0.0));
                }
            }
            travelled += end - start;
            start = 0.0;
        }
        Some((travelled, limit, next_turn))
    }
}

#[derive(Default)]
pub(super) struct ScheduleRunner {
    speed: f64,
    route: Option<Route>,
    revision: u64,
    retry_at: u64,
}

impl ScheduleRunner {
    pub(super) fn moving(&self) -> bool {
        self.speed != 0.0
    }

    /// Returns whether the timetable changed and whether the train moved.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn tick(
        &mut self,
        world: World<'_>,
        content: Content,
        time: u64,
        network: &RailwayData,
        signals: &SignalPlan<'_>,
        train: &mut Train,
        available: bool,
    ) -> (bool, bool) {
        let Some(schedule) = train.schedule.as_mut() else {
            return (false, false);
        };
        if schedule.waiting > 0 {
            schedule.waiting -= 1;
            return (true, false);
        }
        let Some(destination) = schedule.destination().cloned() else {
            return (false, false);
        };
        if self.revision != railway::revision() {
            self.route = None;
            self.revision = railway::revision();
            self.retry_at = 0;
        }
        let Some(graph) = train.graph(network) else {
            self.speed = 0.0;
            return (false, false);
        };
        let Some(wheel) = train.end_wheel(true) else {
            return (false, false);
        };
        if self.route.is_none() {
            if !available || time < self.retry_at {
                return (false, false);
            }
            self.retry_at = time + RETRY_TICKS;
            if !station::can_schedule(world, content, &destination) {
                return (false, false);
            }
            self.route = station_point(world, content, network, &destination)
                .and_then(|(edge, distance)| Route::new(graph, wheel, edge, distance));
        }
        let Some(route) = self.route.as_mut() else {
            return (false, false);
        };
        let Some(edge) = route.leading_edge(graph, wheel) else {
            self.route = None;
            self.speed = 0.0;
            return (false, false);
        };
        let red = signals.blocking(graph, train.id, true);
        let Some((left, red, turn)) = route.distances(graph, edge, wheel.offset(), &red) else {
            self.route = None;
            self.speed = 0.0;
            return (false, false);
        };
        if left <= EPSILON && available {
            train.stopped_at = Some(destination);
            if let Some(schedule) = train.schedule.as_mut() {
                schedule.arrive();
            }
            *self = Self::default();
            return (true, false);
        }
        let braking = self.speed.powi(2) / (2.0 * ACCELERATION);
        let stop = if available {
            left
        } else {
            (left - SIGNAL_CLEARANCE).max(0.0)
        }
        .min(red);
        let mut target =
            if turn <= braking - TURN_SPEED.powi(2) / (2.0 * ACCELERATION) || turn == 0.0 {
                TURN_SPEED
            } else {
                TOP_SPEED
            };
        if stop <= braking {
            target = 0.0;
        }
        self.speed += (target - self.speed).clamp(-ACCELERATION, ACCELERATION);
        let step = self.speed.min(stop);
        if step == 0.0 {
            self.speed = 0.0;
            return (false, false);
        }
        // Keep the timetable running even with no player loading the carriage's chunk.
        let moved = train.advance(None, graph, step, 0.0, |edge, _| route.next(graph, edge));
        if moved.is_none_or(|moved| (step - moved).abs() > EPSILON) || step >= stop {
            self.speed = 0.0;
        }
        let changed = moved.is_some_and(|distance| distance > 0.0);
        (changed, changed)
    }
}
