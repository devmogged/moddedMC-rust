//! AsyncDragonPathfinder / AsyncFlightPathSearch (0.9.88), advanced on content ticks.
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, HashSet, VecDeque},
    sync::{LazyLock, Mutex, PoisonError},
    time::{Duration, Instant},
};

use moddedmc_api::{Pos, abi::CollisionBox, content::World, entities::Entity, tick_count};

use crate::flight::{add, cell, length_squared, offset};

type Point = [f64; 3];
type Key = [i32; 3];
const MAX_VISITED: usize = 5000;
const MAX_ROUTE: f64 = 128.0;
const MAX_SHORTCUT: f64 = 24.0;
const SWEEP_LENGTH: f64 = 4.0;
const WEIGHT: f64 = 1.25;
const GOAL_SWEEP_INTERVAL: usize = 16;
const EPS: f64 = 1.0e-7;
// One content tick advances at most this many search steps across all dimensions.
const TICK_STEPS: usize = 16;
const TICK_TIME: Duration = Duration::from_micros(350);
const TICK_CELL_READS: usize = 128;
#[derive(Clone, Copy)]
struct TickLimits {
    steps: usize,
    time: Duration,
    cell_reads: usize,
}
const TICK_LIMITS: TickLimits = TickLimits {
    steps: TICK_STEPS,
    time: TICK_TIME,
    cell_reads: TICK_CELL_READS,
};
const MAX_REQUESTS: usize = 34; // Java's two workers + 32 waiting requests.
const LEASE: u64 = 40;

fn key(p: Point) -> Key {
    p.map(|v| v.floor() as i32)
}
fn center(p: Key) -> Point {
    p.map(|v| f64::from(v) + 0.5)
}
fn distance(a: Point, b: Point) -> f64 {
    length_squared(offset(a, b)).sqrt()
}
fn scale(p: Point, n: f64) -> Point {
    p.map(|v| v * n)
}
fn neighbors(p: Key) -> impl Iterator<Item = Key> {
    (-1..=1).flat_map(move |x| {
        (-1..=1).flat_map(move |y| {
            (-1..=1).filter_map(move |z| {
                (x != 0 || y != 0 || z != 0).then_some([p[0] + x, p[1] + y, p[2] + z])
            })
        })
    })
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Point,
    max: Point,
}
impl Bounds {
    fn at(self, p: Point) -> Self {
        Self {
            min: add(self.min, p),
            max: add(self.max, p),
        }
    }
}

/// VoxelAabbSweeper.collidesDuringSweep: continuous intersection, not sampled motion.
fn collides(b: Bounds, movement: Point, obstacle: Bounds) -> bool {
    if (0..3).all(|i| b.min[i] < obstacle.max[i] && b.max[i] > obstacle.min[i]) {
        return true;
    }
    let mut entry = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    for (i, v) in movement.into_iter().enumerate() {
        if v.abs() <= EPS {
            if b.max[i] <= obstacle.min[i] + EPS || b.min[i] >= obstacle.max[i] - EPS {
                return false;
            }
        } else {
            let a = (obstacle.min[i] - b.max[i]) / v;
            let z = (obstacle.max[i] - b.min[i]) / v;
            entry = entry.max(a.min(z));
            exit = exit.min(a.max(z));
        }
    }
    entry <= exit + EPS && exit > EPS && entry < 1.0 - EPS
}

// A yielded query has no collision result. Retry it using the cached cells next tick.
type Probe = Result<bool, ()>;

fn sweep(
    b: Bounds,
    movement: Point,
    mut blocked: impl FnMut(Pos, Bounds, Point) -> Probe,
) -> Probe {
    let end = b.at(movement);
    let min = cell(std::array::from_fn(|i| b.min[i].min(end.min[i])));
    let max = cell(std::array::from_fn(|i| b.max[i].max(end.max[i])));
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                if blocked(Pos { x, y, z }, b, movement)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn block_collision(pos: Pos, b: Bounds, movement: Point, shapes: Option<&[CollisionBox]>) -> bool {
    let at = [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)];
    match shapes {
        Some(shapes) => shapes.iter().any(|s| {
            collides(
                b,
                movement,
                Bounds {
                    min: add(at, s.min),
                    max: add(at, s.max),
                },
            )
        }),
        None => collides(
            b,
            movement,
            Bounds {
                min: at,
                max: add(at, [1.0; 3]),
            },
        ),
    }
}

#[derive(Clone, Copy)]
struct Open {
    key: Key,
    g: f64,
    h: f64,
}
impl Open {
    fn f(self) -> f64 {
        self.g + WEIGHT * self.h
    }
}
impl PartialEq for Open {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f()
            .total_cmp(&self.f())
            .then_with(|| other.h.total_cmp(&self.h))
    }
}

struct Search {
    origin: Point,
    target: Point,
    requested: Point,
    start: Key,
    goal: Key,
    min: Key,
    max: Key,
    bounds: Bounds,
    open: BinaryHeap<Open>,
    parent: HashMap<Key, Key>,
    g: HashMap<Key, f64>,
    closed: HashSet<Key>,
    clear_nodes: HashMap<Key, bool>,
    clear_edges: HashMap<(Key, Key), bool>,
    best: Key,
    best_h: f64,
    visited: usize,
    direct: bool,
}

struct Route {
    points: VecDeque<Point>,
    reached: bool,
    direct: bool,
}

impl Search {
    fn new(origin: Point, requested: Point, bounds: Bounds, height: (i32, i32)) -> Self {
        let delta = offset(requested, origin);
        let target = add(
            origin,
            scale(delta, (MAX_ROUTE / distance(origin, requested)).min(1.0)),
        );
        let (start, goal) = (key(origin), key(target));
        let width = bounds.max[0] - bounds.min[0];
        let h = bounds.max[1] - bounds.min[1];
        let horizontal = 12.max(width.ceil() as i32 + 6);
        let vertical = 8.max(h.ceil() as i32 + 4);
        let margin = [horizontal, vertical, horizontal];
        let mut min = std::array::from_fn(|i| start[i].min(goal[i]) - margin[i]);
        let mut max = std::array::from_fn(|i| start[i].max(goal[i]) + margin[i]);
        min[1] = min[1].max(height.0);
        max[1] = max[1].min(height.1 - 1);
        let best_h = distance(origin, if start == goal { origin } else { center(goal) });
        Self {
            origin,
            target,
            requested,
            start,
            goal,
            min,
            max,
            bounds,
            open: BinaryHeap::from([Open {
                key: start,
                g: 0.0,
                h: best_h,
            }]),
            parent: HashMap::new(),
            g: HashMap::from([(start, 0.0)]),
            closed: HashSet::new(),
            clear_nodes: HashMap::new(),
            clear_edges: HashMap::new(),
            best: start,
            best_h,
            visited: 0,
            direct: true,
        }
    }
    fn position(&self, p: Key) -> Point {
        if p == self.start {
            self.origin
        } else {
            center(p)
        }
    }
    fn within(&self, p: Key) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }
    fn heuristic(&self, p: Key) -> f64 {
        distance(self.position(p), self.position(self.goal))
    }
    fn segment(
        &self,
        from: Point,
        to: Point,
        clear: &mut impl FnMut(Bounds, Point) -> Probe,
    ) -> Probe {
        let movement = offset(to, from);
        let steps = (distance(from, to) / SWEEP_LENGTH).ceil().max(1.0) as usize;
        let mut cursor = from;
        for i in 1..=steps {
            let end = if i == steps {
                to
            } else {
                add(from, scale(movement, i as f64 / steps as f64))
            };
            if !clear(self.bounds.at(cursor), offset(end, cursor))? {
                return Ok(false);
            }
            cursor = end;
        }
        Ok(true)
    }
    fn edge_key(a: Key, b: Key) -> (Key, Key) {
        if a < b { (a, b) } else { (b, a) }
    }
    fn edge(&mut self, a: Key, b: Key, clear: &mut impl FnMut(Bounds, Point) -> Probe) -> Probe {
        let edge = Self::edge_key(a, b);
        if let Some(&ok) = self.clear_edges.get(&edge) {
            return Ok(ok);
        }
        let ok = self.segment(self.position(a), self.position(b), clear)?;
        self.clear_edges.insert(edge, ok);
        Ok(ok)
    }
    fn finish(&self, end: Key, reached: bool) -> Route {
        let mut points = VecDeque::new();
        let mut current = end;
        while current != self.start {
            points.push_front(self.position(current));
            let Some(&previous) = self.parent.get(&current) else {
                break;
            };
            current = previous;
        }
        if reached {
            points.push_back(self.target);
        }
        Route {
            points,
            reached: reached && distance(self.target, self.requested) < 1.0e-4,
            direct: self.direct,
        }
    }

    /// One confirmed expansion (or direct-route probe); all returned edges have been swept.
    fn step(
        &mut self,
        clear: &mut impl FnMut(Bounds, Point) -> Probe,
    ) -> Result<Option<Route>, ()> {
        if self.direct {
            if self.goal != self.start
                && self.within(self.goal)
                && self.segment(self.origin, self.position(self.goal), clear)?
                && self.segment(self.position(self.goal), self.target, clear)?
            {
                self.parent.insert(self.goal, self.start);
                return Ok(Some(self.finish(self.goal, true)));
            }
            self.direct = false;
        }
        if self.visited >= MAX_VISITED {
            return Ok(Some(self.finish(self.best, false)));
        }
        let Some(current) = self.open.peek().copied() else {
            return Ok(Some(self.finish(self.best, false)));
        };
        let p = current.key;
        let mut known = self.g.get(&p).copied().unwrap_or(f64::INFINITY);
        if current.g != known || self.closed.contains(&p) {
            self.open.pop();
            return Ok(None);
        }
        if let Some(parent) = self.parent.get(&p).copied()
            && !self.edge(parent, p, clear)?
        {
            let mut repair: Option<(Key, f64)> = None;
            for n in neighbors(p) {
                if !self.closed.contains(&n) {
                    continue;
                }
                let score = self.g[&n] + distance(self.position(n), self.position(p));
                if repair.is_none_or(|(_, g)| score < g) && self.edge(n, p, clear)? {
                    repair = Some((n, score));
                }
            }
            let Some((parent, score)) = repair else {
                self.open.pop();
                self.parent.remove(&p);
                self.g.remove(&p);
                return Ok(None);
            };
            self.parent.insert(p, parent);
            self.g.insert(p, score);
            if score != known {
                self.open.pop();
                self.open.push(Open {
                    key: p,
                    g: score,
                    h: current.h,
                });
                return Ok(None);
            }
            known = score;
        }
        let h = self.heuristic(p);
        if p == self.goal && self.segment(self.position(p), self.target, clear)? {
            self.visited += 1;
            return Ok(Some(self.finish(p, true)));
        }
        if (self.visited + 1).is_multiple_of(GOAL_SWEEP_INTERVAL)
            && !self.closed.contains(&self.goal)
            && self.within(self.goal)
            && self.segment(self.position(p), self.position(self.goal), clear)?
            && self.segment(self.position(self.goal), self.target, clear)?
        {
            self.visited += 1;
            self.parent.insert(self.goal, p);
            return Ok(Some(self.finish(self.goal, true)));
        }
        // Finish every potentially yielding query before committing this expansion.
        for next in neighbors(p) {
            if !self.within(next)
                || self.closed.contains(&next)
                || self.clear_nodes.contains_key(&next)
            {
                continue;
            }
            let ok = clear(self.bounds.at(center(next)), [0.0; 3])?;
            self.clear_nodes.insert(next, ok);
        }
        self.open.pop();
        self.closed.insert(p);
        self.visited += 1;
        if h < self.best_h {
            self.best = p;
            self.best_h = h;
        }
        for next in neighbors(p) {
            if !self.within(next)
                || self.closed.contains(&next)
                || self.clear_nodes.get(&next) != Some(&true)
            {
                continue;
            }
            let mut score = known + distance(self.position(p), self.position(next));
            let mut parent = p;
            if let Some(&ancestor) = self.parent.get(&p) {
                let length = distance(self.position(ancestor), self.position(next));
                let shortcut = self.g[&ancestor] + length;
                if length <= MAX_SHORTCUT
                    && shortcut <= score
                    && self.clear_edges.get(&Self::edge_key(ancestor, next)) != Some(&false)
                {
                    score = shortcut;
                    parent = ancestor;
                }
            }
            if score >= self.g.get(&next).copied().unwrap_or(f64::INFINITY) {
                continue;
            }
            self.parent.insert(next, parent);
            self.g.insert(next, score);
            self.open.push(Open {
                key: next,
                g: score,
                h: self.heuristic(next),
            });
        }
        Ok(None)
    }
}

struct Request {
    dimension: &'static str,
    target: Point,
    touched: u64,
    submitted: u64,
    swarm: bool,
    search: Option<Search>,
    route: Option<Route>,
    cells: HashMap<Pos, Option<Box<[CollisionBox]>>>,
    sweeps: HashMap<[u64; 6], bool>,
}
#[derive(Default)]
struct Planner {
    requests: HashMap<i32, Request>,
    queue: VecDeque<i32>,
    tick: u64,
    steps: usize,
    elapsed: Duration,
    cell_reads: usize,
}
static PLANNER: LazyLock<Mutex<Planner>> = LazyLock::new(|| Mutex::new(Planner::default()));

pub fn forget(id: i32) {
    let mut planner = PLANNER.lock().unwrap_or_else(PoisonError::into_inner);
    planner.requests.remove(&id);
    planner.queue.retain(|&entry| entry != id);
}

/// Request or consume a route. Until a result arrives, the caller retains its short
/// local collision guard. A missing or exhausted partial route never authorizes motion.
pub fn waypoint(world: World<'_>, mob: Entity<'_>, at: Point, target: Point, swarm: bool) -> Point {
    let (Some(dimension), Some(body)) = (world.dimension_id(), mob.body()) else {
        return target;
    };
    let id = mob.id();
    let now = tick_count();
    let mut planner = PLANNER.lock().unwrap_or_else(PoisonError::into_inner);
    let replace = planner.requests.get(&id).is_none_or(|r| {
        r.dimension != dimension
            || r.swarm != swarm
            || (r.search.is_none()
                && now.saturating_sub(r.submitted) >= 8
                && distance(r.target, target) > if swarm { 1.0 } else { 4.0 })
            || distance(r.target, target) > 24.0
            || (r.search.is_none()
                && now.saturating_sub(r.submitted) >= 20
                && r.route
                    .as_ref()
                    .is_none_or(|p| p.points.is_empty() && !p.reached))
            || (r.search.is_none() && now.saturating_sub(r.submitted) >= 100)
    });
    if replace {
        let pending = planner
            .requests
            .values()
            .filter(|r| r.search.is_some())
            .count();
        if pending < MAX_REQUESTS
            || planner
                .requests
                .get(&id)
                .is_some_and(|r| r.search.is_some())
        {
            let bounds = Bounds {
                min: offset(body.min, at),
                max: offset(body.max, at),
            };
            let height = moddedmc_api::worldgen_lookup::require()
                .ok()
                .and_then(|lookup| lookup.height_bounds(world))
                .unwrap_or((-64, 320));
            planner.requests.insert(
                id,
                Request {
                    dimension,
                    target,
                    touched: now,
                    submitted: now,
                    swarm,
                    search: Some(Search::new(at, target, bounds, height)),
                    route: None,
                    cells: HashMap::new(),
                    sweeps: HashMap::new(),
                },
            );
            planner.queue.retain(|&entry| entry != id);
            planner.queue.push_back(id);
        }
    }
    let Some(request) = planner.requests.get_mut(&id) else {
        return target;
    };
    request.touched = now;
    let Some(route) = request.route.as_mut() else {
        return target;
    };
    if route.direct && route.reached {
        return target;
    }
    let arrival = if swarm { 0.6 } else { 0.8 };
    while route
        .points
        .front()
        .is_some_and(|&p| distance(at, p) <= arrival)
    {
        route.points.pop_front();
    }
    route
        .points
        .front()
        .copied()
        .unwrap_or(if route.reached { target } else { at })
}

/// Reject a stale result when the immediate movement is obstructed; leave a pending
/// search intact so a stopped controller can still receive its detour.
pub fn blocked(id: i32) {
    let mut planner = PLANNER.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(request) = planner.requests.get_mut(&id)
        && request.search.is_none()
    {
        request.route = None;
        request.submitted = tick_count().saturating_sub(20);
    }
}

/// Advance pending searches outside the movement callbacks, round-robin, with both a
/// work cap, a host cell-read cap and a time limit checked between host queries.
/// The host exposes live cells rather than immutable
/// snapshots; cache each cell on first access, and keep the controller's local check.
pub fn tick(world: World<'_>, now: u64) {
    let Some(dimension) = world.dimension_id() else {
        return;
    };
    let mut planner = PLANNER.lock().unwrap_or_else(PoisonError::into_inner);
    let mut pellucida = None;
    advance(&mut planner, dimension, now, TICK_LIMITS, |pos, swarm| {
        world.collision_shape(pos).map(|shapes| {
            if !shapes.is_empty()
                && swarm
                && pellucida
                    .get_or_insert_with(|| {
                        crate::CONTENT
                            .get()
                            .and_then(|c| c.state("saintsdragons:draconian_pellucida"))
                    })
                    .is_some_and(|id| {
                        let mut state = [moddedmc_api::content::UNLOADED_STATE];
                        world.read(&[pos], &mut state) && state[0] == id
                    })
            {
                return Box::<[CollisionBox]>::default();
            }
            shapes.into_boxed_slice()
        })
    });
}

fn advance(
    planner: &mut Planner,
    dimension: &str,
    now: u64,
    limits: TickLimits,
    mut read_shapes: impl FnMut(Pos, bool) -> Option<Box<[CollisionBox]>>,
) {
    if planner.tick != now {
        planner.tick = now;
        planner.steps = 0;
        planner.elapsed = Duration::ZERO;
        planner.cell_reads = 0;
        planner
            .requests
            .retain(|_, r| now.saturating_sub(r.touched) <= LEASE);
        let Planner {
            requests, queue, ..
        } = &mut *planner;
        queue.retain(|id| requests.contains_key(id));
    }
    if planner.queue.is_empty() {
        return;
    }
    let started = Instant::now();
    let deadline = started + limits.time.saturating_sub(planner.elapsed);
    let mut skipped = 0;
    while planner.steps < limits.steps
        && planner.cell_reads < limits.cell_reads
        && Instant::now() < deadline
    {
        let Some(id) = planner.queue.pop_front() else {
            break;
        };
        let mut reads = planner.cell_reads;
        let Some(r) = planner.requests.get_mut(&id) else {
            continue;
        };
        if r.dimension != dimension {
            planner.queue.push_back(id);
            skipped += 1;
            if skipped >= planner.queue.len() {
                break;
            }
            continue;
        }
        skipped = 0;
        let Some(search) = r.search.as_mut() else {
            continue;
        };
        let result = search.step(&mut |b, movement| {
            let sweep_key = std::array::from_fn(|i| {
                if i < 3 {
                    b.min[i].to_bits()
                } else {
                    movement[i - 3].to_bits()
                }
            });
            if let Some(&ok) = r.sweeps.get(&sweep_key) {
                return Ok(ok);
            }
            if Instant::now() >= deadline {
                return Err(());
            }
            let ok = sweep(b, movement, |pos, b, movement| {
                if let std::collections::hash_map::Entry::Vacant(entry) = r.cells.entry(pos) {
                    if reads >= limits.cell_reads || Instant::now() >= deadline {
                        return Err(());
                    }
                    reads += 1;
                    let shapes = read_shapes(pos, r.swarm);
                    entry.insert(shapes);
                }
                Ok(block_collision(pos, b, movement, r.cells[&pos].as_deref()))
            })?;
            r.sweeps.insert(sweep_key, ok);
            Ok(ok)
        });
        let yielded = result.is_err();
        if let Ok(Some(route)) = result {
            r.route = Some(route);
            r.search = None;
            r.cells = HashMap::new();
            r.sweeps = HashMap::new();
        } else {
            planner.queue.push_back(id);
        }
        planner.steps += 1;
        planner.cell_reads = reads;
        if yielded {
            break;
        }
    }
    planner.elapsed += started.elapsed();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_dragon_tick_cost() {
        measure_eight_dragon_ticks(TickLimits {
            steps: 32,
            cell_reads: 512,
            time: Duration::from_millis(2),
        });
        measure_eight_dragon_ticks(TICK_LIMITS);
    }

    fn measure_eight_dragon_ticks(limits: TickLimits) {
        let mut planner = Planner::default();
        let dimension = "minecraft:overworld";
        let bounds = Bounds {
            min: [-0.9, 0.0, -0.9],
            max: [0.9, 2.0, 0.9],
        };
        for id in 0..8 {
            let z = f64::from(id) * 12.0 + 0.5;
            let origin = [0.5, 8.5, z];
            let target = [40.5, 8.5, z];
            planner.requests.insert(
                id,
                Request {
                    dimension,
                    target,
                    touched: 0,
                    submitted: 0,
                    swarm: false,
                    search: Some(Search::new(origin, target, bounds, (-64, 320))),
                    route: None,
                    cells: HashMap::new(),
                    sweeps: HashMap::new(),
                },
            );
            planner.queue.push_back(id);
        }
        let mut times = Vec::new();
        for now in 1..=4000 {
            for r in planner.requests.values_mut() {
                r.touched = now;
            }
            let start = Instant::now();
            advance(&mut planner, dimension, now, limits, |pos, _| {
                let solid = pos.x == 18 && (5..=12).contains(&pos.y);
                Some(if solid {
                    vec![CollisionBox {
                        min: [0.0; 3],
                        max: [1.0; 3],
                    }]
                    .into_boxed_slice()
                } else {
                    Box::default()
                })
            });
            times.push(start.elapsed());
            assert!(planner.steps <= limits.steps);
            assert!(planner.cell_reads <= limits.cell_reads);
            if planner.queue.is_empty() {
                break;
            }
        }
        assert!(planner.queue.is_empty(), "eight requests must finish");
        assert_eq!(
            planner
                .requests
                .values()
                .filter(|r| r.route.as_ref().is_some_and(|p| p.reached))
                .count(),
            8
        );
        times.sort_unstable();
        let mean = times.iter().sum::<Duration>() / times.len() as u32;
        eprintln!(
            "8-dragon planner ({} steps / {} cells / {} us): {} active ticks, mean {:.3} ms, median {:.3} ms, p95 {:.3} ms, max {:.3} ms (synthetic cells, excludes ABI latency)",
            limits.steps,
            limits.cell_reads,
            limits.time.as_micros(),
            times.len(),
            mean.as_secs_f64() * 1000.0,
            times[times.len() / 2].as_secs_f64() * 1000.0,
            times[(times.len() - 1) * 95 / 100].as_secs_f64() * 1000.0,
            times.last().unwrap().as_secs_f64() * 1000.0
        );
    }

    #[test]
    fn routes_around_wall_with_bounded_expansions_and_confirmed_edges() {
        let bounds = Bounds {
            min: [-0.3, 0.0, -0.3],
            max: [0.3, 0.8, 0.3],
        };
        let wall = Bounds {
            min: [4.0, -2.0, -2.0],
            max: [5.0, 4.0, 2.0],
        };
        let mut clear = |b, m| Ok(!collides(b, m, wall));
        let mut search = Search::new([0.5, 0.5, 0.5], [10.5, 0.5, 0.5], bounds, (-64, 320));
        let mut route = None;
        for _ in 0..MAX_VISITED * 4 {
            if let Some(result) = search.step(&mut clear).unwrap() {
                route = Some(result);
                break;
            }
        }
        let route = route.expect("bounded search");
        assert!(route.reached);
        assert!(search.visited < 300, "visited {}", search.visited);
        assert!(route.points.len() >= 2);
        let mut from = search.origin;
        for to in route.points {
            assert!(search.segment(from, to, &mut clear).unwrap());
            from = to;
        }
        assert_eq!(from, search.requested);
    }

    #[test]
    fn clear_route_and_truncated_route_use_java_limit() {
        let bounds = Bounds {
            min: [-0.5, 0.0, -0.5],
            max: [0.5, 1.0, 0.5],
        };
        let mut search = Search::new([0.5; 3], [200.5, 0.5, 0.5], bounds, (-64, 320));
        let route = search.step(&mut |_, _| Ok(true)).unwrap().unwrap();
        assert!(!route.reached);
        assert_eq!(route.points.back(), Some(&[128.5, 0.5, 0.5]));
        assert_eq!(search.visited, 0);
    }

    #[test]
    fn swept_boxes_detect_thin_obstacles_but_not_touching_or_moving_away() {
        let body = Bounds {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        let obstacle = Bounds {
            min: [2.0, 0.0, 0.0],
            max: [2.1, 1.0, 1.0],
        };
        assert!(collides(body, [4.0, 0.0, 0.0], obstacle));
        assert!(!collides(body, [1.0, 0.0, 0.0], obstacle));
        assert!(!collides(
            body.at([1.0, 0.0, 0.0]),
            [-1.0, 0.0, 0.0],
            obstacle
        ));
    }
}
