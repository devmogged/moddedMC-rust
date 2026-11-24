//! Create 6.0.10's hose pulley.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet, VecDeque},
};

use fastnbt::{IntArray, Value};
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    block_displays::BlockDisplay,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, State, World},
    fluid_world::FluidCell,
};

use super::{
    network::FluidHandler,
    open_end_runtime::{fluids, inspect, waterlogged_state},
    pipe_connection::FluidStack,
    runtime::hose_pipe_neighbors,
    smart_tank,
    tank::TankInventory,
};
use crate::{
    BLOCK_DISPLAYS, CONTENT, DIRECTIONS, FACES, StateIds, face_axis,
    kinetics::{Kind, Kinetics},
    nearest_looking_direction, offset,
};

pub const ID: &str = "create:hose_pulley";
pub const FACINGS: [u8; 4] = [2, 5, 3, 4];
const CAPACITY: u32 = 1_500;
const BUCKET: u32 = 1_000;
const RANGE: u32 = 128;
const BLOCK_THRESHOLD: usize = 10_000;
const SEARCHED_PER_TICK: usize = 1_024;
const VALIDATION_TICKS: u16 = 160;
const RPM_PER_BLOCK_PER_TICK: f32 = 512.0;
const WORLD_FLAGS: u8 = content::UPDATE_CLIENTS | content::UPDATE_KNOWN_SHAPE;
const CABLE_MODELS: [(&str, &str, &str); 2] = [
    (
        "create:hose_pulley_magnet",
        "Hose Nozzle",
        "create:block/hose_pulley/pulley_magnet",
    ),
    (
        "create:hose_pulley_hose",
        "Hose",
        "create:block/hose_pulley/rope",
    ),
];

pub const fn shaft_side(facing: u8) -> u8 {
    match facing {
        2 => 5,
        5 => 3,
        3 => 4,
        _ => 2,
    }
}

pub const fn pipe_side(facing: u8) -> u8 {
    shaft_side(facing) ^ 1
}

pub fn state(content: Content, facing: u8) -> Option<u32> {
    content.state(&format!("{ID}[facing={}]", DIRECTIONS[usize::from(facing)]))
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let properties = FACINGS.map(|face| format!("facing={}", DIRECTIONS[usize::from(face)]));
    let states: [State<'_>; 4] = std::array::from_fn(|index| State {
        properties: &properties[index],
        physical: "minecraft:copper_block",
        model: "create:block/hose_pulley/block",
        rotating_model: "create:block/hose_pulley/hose_coil",
        x: 0,
        y: (index as i16) * 90,
        orientation: crate::motor_orientation(FACINGS[index] ^ 1),
        shape: &[],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Hose Pulley",
            entity: ID,
            item_model: "create:item/hose_pulley",
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    for (id, display_name, model) in CABLE_MODELS {
        content.add_block(
            registry,
            &Block {
                id,
                display_name,
                entity: "",
                item_model: "",
                states: &[State {
                    properties: "",
                    physical: "minecraft:copper_block",
                    model,
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }],
            },
            None,
        )?;
    }
    Ok(())
}

pub fn placed_state(args: &Placement<'_>, neighbors: &[u32; 7], ids: &StateIds) -> Option<u32> {
    let axis_of = |open: [bool; 6]| {
        let mut faces = (2..6u8).filter(|&face| open[usize::from(face)]);
        let first = faces.next()?;
        faces.try_fold(first, |last, face| {
            (face_axis(last) == face_axis(face)).then_some(face)
        })
    };
    let shafts = std::array::from_fn(|face| {
        ids.kind(neighbors[face])
            .is_some_and(|kind| kind.has_shaft_towards(face as u8 ^ 1))
    });
    let preferred = axis_of(shafts)
        .or_else(|| axis_of(hose_pipe_neighbors(args.world, args.position)?).map(|pipe| pipe ^ 1));
    let facing = preferred
        .map(|shaft| shaft_side(shaft) ^ 1)
        .unwrap_or_else(|| nearest_looking_direction(args.yaw, 0.0) ^ 1);
    state(*CONTENT.get()?, facing)
}

#[derive(Default)]
pub struct HosePulleys {
    blocks: HashMap<Pos, HosePulley>,
    resets: VecDeque<AreaScan>,
}

impl HosePulleys {
    pub(super) fn get(&self, pos: &Pos) -> Option<&HosePulley> {
        self.blocks.get(pos)
    }

    pub(super) fn get_mut(&mut self, pos: &Pos) -> Option<&mut HosePulley> {
        self.blocks.get_mut(pos)
    }

    fn reset(&mut self, affected: AffectedArea) {
        self.resets.push_back(affected.scan());
    }
}

pub fn event(world: World<'_>, pulleys: &mut HosePulleys, event: &ContentEvent) {
    if event.reason != content::UNLOADED && is_pulley(event.state) {
        pulleys
            .blocks
            .entry(event.position)
            .or_insert_with(|| HosePulley::load(world, event.position));
    } else if is_pulley(event.previous)
        && let Some(mut pulley) = pulleys.blocks.remove(&event.position)
    {
        pulley.clear_cable(world);
        if event.reason != content::UNLOADED
            && let Some(area) = pulley.affected
        {
            pulleys.reset(area);
        }
    }
}

fn is_pulley(state: u32) -> bool {
    crate::states().is_some_and(|ids| matches!(ids.kind(state), Some(Kind::HosePulley(_))))
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    for pos in kinetics.positions(|kind| matches!(kind, Kind::HosePulley(_))) {
        if let Some(be) = kinetics.get(pos) {
            let rpm = be.get_speed();
            if let Some(pulley) = kinetics.hose_pulleys.blocks.get_mut(&pos)
                && let Some(area) = pulley.tick(world, pos, rpm)
            {
                kinetics.hose_pulleys.reset(area);
            }
        }
    }
    let Some(fluids) = fluids() else { return };
    let pending = &mut kinetics.hose_pulleys.resets;
    for _ in 0..SEARCHED_PER_TICK {
        let Some(area) = pending.front_mut() else {
            break;
        };
        let Some(pos) = area.next() else {
            pending.pop_front();
            continue;
        };
        if let Some(id) = world.fluid_id(pos)
            && id != "minecraft:empty"
        {
            fluids.schedule_tick(world, pos, &id, crate::belt_inventory::next_int(5) as u32);
        }
    }
}

pub fn save_kinetics(world: World<'_>, pos: Pos, kinetic: &crate::KineticData) {
    if let Some(data) = smart_tank::read(world, pos)
        && let Ok(bytes) = fastnbt::to_bytes(kinetic)
        && let Ok(mut updated) = fastnbt::from_bytes::<HashMap<String, Value>>(&bytes)
    {
        updated.extend(
            data.into_iter()
                .filter(|(key, _)| matches!(key.as_str(), "Offset" | "Tank" | "AffectedArea")),
        );
        smart_tank::write(world, pos, &updated);
    }
}

#[derive(Default)]
pub struct HosePulley {
    offset: f32,
    speed: f32,
    moving: bool,
    drain: Option<DrainSearch>,
    fill: Option<FillSearch>,
    affected: Option<AffectedArea>,
    displays: [i32; 2],
    shown_offset: f32,
}

impl HosePulley {
    fn load(world: World<'_>, position: Pos) -> Self {
        let data = smart_tank::read(world, position);
        let offset = data
            .as_ref()
            .and_then(|tag| {
                let Value::Compound(offset) = tag.get("Offset")? else {
                    return None;
                };
                match offset.get("Value") {
                    Some(Value::Float(value)) if value.is_finite() && *value >= 0.0 => Some(*value),
                    _ => None,
                }
            })
            .unwrap_or_default();
        Self {
            offset,
            affected: data.as_ref().and_then(AffectedArea::read),
            ..Self::default()
        }
    }

    fn root(&self, position: Pos) -> Pos {
        below(position, self.offset)
    }

    fn tick(&mut self, world: World<'_>, position: Pos, rpm: f32) -> Option<AffectedArea> {
        let speed = rpm / RPM_PER_BLOCK_PER_TICK;
        let previous_root = self.root(position);
        let attempted = (self.offset + speed).max(0.0);
        let clear = inspect(world, below(position, attempted)).is_some_and(|cell| cell.replaceable);
        self.moving = clear && attempted != self.offset;
        let next = if speed == 0.0 {
            if self.speed == 0.0 {
                self.offset
            } else {
                self.offset.round()
            }
        } else if clear {
            attempted
        } else {
            attempted.floor()
        };
        self.speed = speed;
        if self.offset != next
            && let Some(mut data) = smart_tank::read(world, position)
        {
            data.insert(
                "Offset".to_owned(),
                Value::Compound(HashMap::from([
                    ("Value".to_owned(), Value::Float(next)),
                    ("Target".to_owned(), Value::Float(next)),
                    ("Speed".to_owned(), Value::Float(0.0)),
                ])),
            );
            if smart_tank::write(world, position, &data) {
                self.offset = next;
            }
        }
        self.show_cable(world, position);
        let root = self.root(position);
        if self.moving || root == position {
            self.drain = None;
            self.fill = None;
            return self.reset_area(world, position);
        }
        let Some(cell) = inspect(world, root) else {
            self.drain = None;
            self.fill = None;
            return None;
        };
        if let Some(fluid) = cell.fluid {
            if self.drain.as_ref().is_none_or(|search| {
                search.root != root || search.fluid != fluid || search.expired()
            }) {
                self.drain = Some(DrainSearch::new(root, fluid, false));
            }
            if let Some(search) = self.drain.as_mut() {
                search.tick(world);
            }
        } else {
            self.drain = None;
        }
        if let Some(search) = self.fill.as_mut() {
            if search.root == root && search.revalidate_in != 0 {
                search.tick(world);
            } else {
                self.fill = None;
            }
        }
        if root != previous_root
            || self.affected.is_some_and(|area| {
                !area.depositing
                    && self.drain.as_ref().is_none_or(|search| {
                        search.frontier.is_empty() && search.sources.is_empty()
                    })
            })
        {
            self.reset_area(world, position)
        } else {
            None
        }
    }

    fn clear_cable(&mut self, world: World<'_>) {
        if let Some(displays) = BLOCK_DISPLAYS.get() {
            for id in self.displays.iter_mut().filter(|id| **id != 0) {
                displays.remove(world, std::mem::take(id));
            }
        }
    }

    fn show_cable(&mut self, world: World<'_>, pos: Pos) {
        if self.offset == 0.0 {
            self.clear_cable(world);
            self.shown_offset = 0.0;
            return;
        }
        if self.shown_offset == self.offset && self.displays.iter().all(|id| *id != 0) {
            return;
        }
        let Some((displays, content)) = BLOCK_DISPLAYS.get().zip(CONTENT.get()) else {
            return;
        };
        for (((selector, _, _), scale), id) in CABLE_MODELS
            .into_iter()
            .zip([1.0, self.offset])
            .zip(&mut self.displays)
        {
            let shown = BlockDisplay {
                position: [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5),
                translation: [-0.5, -0.5 - self.offset, -0.5],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, scale, 1.0],
                transform_interpolation: 1,
                position_interpolation: 0,
                brightness: -1,
            };
            if (*id == 0 || !displays.update(world, *id, &shown))
                && let Some(state) = content.state(selector)
            {
                *id = displays.spawn(world, state, &shown).unwrap_or_default();
            }
        }
        self.shown_offset = self.offset;
    }

    fn reset_area(&mut self, world: World<'_>, position: Pos) -> Option<AffectedArea> {
        let affected = self.affected?;
        let mut data = smart_tank::read(world, position)?;
        data.remove("AffectedArea");
        if smart_tank::write(world, position, &data) {
            self.affected = None;
            return Some(affected);
        }
        None
    }

    fn available(&self, world: World<'_>) -> Option<(FluidStack, Pos, bool)> {
        let search = self.drain.as_ref()?;
        if !search.frontier.is_empty() {
            return None;
        }
        let pos = search.first_source(world)?;
        Some((
            FluidStack {
                id: search.fluid.clone(),
                amount: BUCKET,
                components: None,
            },
            pos,
            search.infinite,
        ))
    }
}

#[derive(Clone, Copy)]
struct AffectedArea {
    min: Pos,
    max: Pos,
    depositing: bool,
}

impl AffectedArea {
    fn read(data: &HashMap<String, Value>) -> Option<Self> {
        let Value::IntArray(values) = data.get("AffectedArea")? else {
            return None;
        };
        let [x1, y1, z1, x2, y2, z2, depositing] = **values else {
            return None;
        };
        let in_range = |min: i32, max: i32| {
            min <= max && max.abs_diff(min) <= 2 * RANGE && min > i32::MIN && max < i32::MAX
        };
        (in_range(x1, x2) && in_range(y1, y2) && in_range(z1, z2)).then_some(Self {
            min: Pos {
                x: x1,
                y: y1,
                z: z1,
            },
            max: Pos {
                x: x2,
                y: y2,
                z: z2,
            },
            depositing: depositing != 0,
        })
    }

    fn include(previous: Option<Self>, change: Change) -> Self {
        let pos = change.position;
        let Some(area) = previous else {
            return Self {
                min: pos,
                max: pos,
                depositing: change.depositing,
            };
        };
        Self {
            min: Pos {
                x: area.min.x.min(pos.x),
                y: area.min.y.min(pos.y),
                z: area.min.z.min(pos.z),
            },
            max: Pos {
                x: area.max.x.max(pos.x),
                y: area.max.y.max(pos.y),
                z: area.max.z.max(pos.z),
            },
            depositing: change.depositing,
        }
    }

    fn write(self, data: &mut HashMap<String, Value>) {
        data.insert(
            "AffectedArea".to_owned(),
            Value::IntArray(IntArray::new(vec![
                self.min.x,
                self.min.y,
                self.min.z,
                self.max.x,
                self.max.y,
                self.max.z,
                i32::from(self.depositing),
            ])),
        );
    }

    fn scan(self) -> AreaScan {
        let min = offset(self.min, [-1; 3]);
        AreaScan {
            min,
            max: offset(self.max, [1; 3]),
            next: Some(min),
        }
    }
}

struct AreaScan {
    min: Pos,
    max: Pos,
    next: Option<Pos>,
}

impl Iterator for AreaScan {
    type Item = Pos;

    fn next(&mut self) -> Option<Pos> {
        let pos = self.next?;
        self.next = if pos.x < self.max.x {
            Some(Pos {
                x: pos.x + 1,
                ..pos
            })
        } else if pos.y < self.max.y {
            Some(Pos {
                x: self.min.x,
                y: pos.y + 1,
                ..pos
            })
        } else if pos.z < self.max.z {
            Some(Pos {
                x: self.min.x,
                y: self.min.y,
                z: pos.z + 1,
            })
        } else {
            None
        };
        Some(pos)
    }
}

type SearchKey = (i32, u32, u32, i32, i32);

struct DrainSearch {
    root: Pos,
    fluid: String,
    frontier: VecDeque<(Pos, u32)>,
    visited: HashSet<Pos>,
    sources: BinaryHeap<SearchKey>,
    infinite: bool,
    down: bool,
    revalidate_in: u16,
}

impl DrainSearch {
    fn new(root: Pos, fluid: String, down: bool) -> Self {
        Self {
            root,
            fluid,
            frontier: VecDeque::from([(root, 0)]),
            visited: HashSet::from([root]),
            sources: BinaryHeap::new(),
            infinite: false,
            down,
            revalidate_in: VALIDATION_TICKS,
        }
    }

    fn expired(&self) -> bool {
        self.frontier.is_empty() && self.revalidate_in == 0
    }

    fn is_source(&self, world: World<'_>, pos: Pos) -> bool {
        inspect(world, pos)
            .is_some_and(|cell| cell.source && cell.fluid.as_deref() == Some(self.fluid.as_str()))
    }

    fn first_source(&self, world: World<'_>) -> Option<Pos> {
        let &(y, _, _, x, z) = self.sources.peek()?;
        let pos = Pos { x, y, z };
        (self.infinite || self.is_source(world, pos)).then_some(pos)
    }

    fn tick(&mut self, world: World<'_>) {
        if self.frontier.is_empty() {
            self.revalidate_in = self.revalidate_in.saturating_sub(1);
            if !self.infinite {
                while let Some(&(y, _, _, x, z)) = self.sources.peek() {
                    if self.is_source(world, Pos { x, y, z }) {
                        break;
                    }
                    self.sources.pop();
                }
            }
            return;
        }
        let bottomless = self.down
            || crate::TAGS
                .get()
                .is_some_and(|tags| tags.contains("fluid", "create:bottomless/allow", &self.fluid));
        for _ in 0..SEARCHED_PER_TICK {
            let Some((pos, distance)) = self.frontier.pop_front() else {
                self.visited.clear();
                break;
            };
            let Some(cell) = inspect(world, pos) else {
                continue;
            };
            if cell.fluid.as_deref() != Some(self.fluid.as_str()) {
                continue;
            }
            if cell.source {
                self.sources
                    .push((pos.y, distance, distance_sq(self.root, pos), pos.x, pos.z));
            }
            if bottomless && self.visited.len() >= BLOCK_THRESHOLD && !self.sources.is_empty() {
                self.infinite = true;
                self.frontier.clear();
                self.visited.clear();
                break;
            }
            for (side, direction) in FACES.into_iter().enumerate().skip(usize::from(!self.down)) {
                let next = offset(pos, direction);
                if distance_sq(self.root, next) > RANGE * RANGE || self.visited.contains(&next) {
                    continue;
                }
                let Some(next_cell) = inspect(world, next) else {
                    continue;
                };
                if next_cell.fluid.as_deref() == Some(self.fluid.as_str())
                    && (side != 1
                        || next_cell.source
                        || (next.x == self.root.x && next.z == self.root.z))
                {
                    self.visited.insert(next);
                    self.frontier.push_back((next, distance + 1));
                }
            }
        }
    }
}

fn distance_sq(a: Pos, b: Pos) -> u32 {
    a.x.abs_diff(b.x)
        .saturating_pow(2)
        .saturating_add(a.y.abs_diff(b.y).saturating_pow(2))
        .saturating_add(a.z.abs_diff(b.z).saturating_pow(2))
}

fn below(position: Pos, distance: f32) -> Pos {
    Pos {
        y: position.y.saturating_sub(distance.ceil() as i32),
        ..position
    }
}

pub fn load_tank(world: World<'_>, position: Pos) -> Option<TankInventory> {
    let data = smart_tank::read(world, position)?;
    // Taking a bucket with a partially filled buffer can temporarily exceed capacity.
    let mut tank = TankInventory::new(CAPACITY + BUCKET, 1).ok()?;
    if let Some(content) = data.get("Tank") {
        tank.read_content_nbt(content).ok()?;
    }
    Some(tank)
}

fn save_tank(world: World<'_>, position: Pos, tank: &TankInventory) -> bool {
    let Some(mut data) = smart_tank::read(world, position) else {
        return false;
    };
    data.insert("Tank".to_owned(), tank.content_nbt());
    smart_tank::write(world, position, &data)
}

enum Space {
    Fillable(u32),
    Filled,
    Blocking,
}

fn space(world: World<'_>, position: Pos, id: &str, cell: &FluidCell) -> Option<Space> {
    let content = CONTENT.get()?;
    let same = cell.fluid.as_deref() == Some(id);
    if cell.has_waterlogged_property {
        if id != "minecraft:water" {
            return Some(Space::Blocking);
        }
        if cell.waterlogged {
            return Some(Space::Filled);
        }
        let wet = waterlogged_state(cell, true)?;
        let state = content.describe_state(wet)?;
        let wet = if state.contains("lit=true") {
            content.state(&state.replace("lit=true", "lit=false"))?
        } else {
            wet
        };
        return Some(Space::Fillable(wet));
    }
    if cell.liquid_block {
        return Some(if cell.source {
            if same { Space::Filled } else { Space::Blocking }
        } else {
            Space::Fillable(content.state(id)?)
        });
    }
    if cell.fluid.is_some() && world.collision_shape(position)?.is_empty() {
        return Some(if same { Space::Filled } else { Space::Blocking });
    }
    if cell.blocks_motion {
        return Some(Space::Blocking);
    }
    if cell.replaceable && content.state("minecraft:air") == Some(cell.state) {
        return Some(Space::Fillable(content.state(id)?));
    }
    let selector = content.describe_state(cell.state)?;
    let block = selector.split('[').next()?;
    let except = block.ends_with("_door")
        || crate::TAGS
            .get()?
            .contains("block", "minecraft:all_signs", block)
        || matches!(
            block,
            "minecraft:ladder"
                | "minecraft:sugar_cane"
                | "minecraft:bubble_column"
                | "minecraft:nether_portal"
                | "minecraft:end_portal"
                | "minecraft:end_gateway"
                | "minecraft:structure_void"
        );
    Some(if !except {
        Space::Fillable(content.state(id)?)
    } else {
        Space::Blocking
    })
}

struct FillSearch {
    root: Pos,
    fluid: String,
    queue: BinaryHeap<Reverse<SearchKey>>,
    queued: HashSet<Pos>,
    ready: Option<Change>,
    revalidate_in: u16,
    infinite: bool,
    filled: usize,
    validation: Option<DrainSearch>,
}

impl FillSearch {
    fn new(world: World<'_>, root: Pos, fluid: String) -> Self {
        let validation = inspect(world, root)
            .is_some_and(|cell| cell.fluid.as_deref() == Some(fluid.as_str()))
            .then(|| DrainSearch::new(root, fluid.clone(), true));
        Self {
            root,
            fluid,
            queue: BinaryHeap::from([Reverse((root.y, 0, 0, root.x, root.z))]),
            queued: HashSet::from([root]),
            ready: None,
            revalidate_in: VALIDATION_TICKS,
            infinite: false,
            filled: 0,
            validation,
        }
    }

    fn tick(&mut self, world: World<'_>) {
        self.ready = None;
        self.revalidate_in = self.revalidate_in.saturating_sub(1);
        if self.infinite || !has_block_state(&self.fluid) {
            return;
        }
        if let Some(scan) = self.validation.as_mut() {
            scan.tick(world);
            self.infinite = scan.infinite;
            if scan.frontier.is_empty() {
                self.validation = None;
            }
            if self.infinite || self.validation.is_some() {
                return;
            }
        }
        for _ in 0..SEARCHED_PER_TICK {
            let Some(&Reverse((y, distance, _, x, z))) = self.queue.peek() else {
                break;
            };
            let position = Pos { x, y, z };
            let cell = inspect(world, position);
            let next = cell
                .as_ref()
                .and_then(|cell| space(world, position, &self.fluid, cell));
            match next {
                Some(Space::Fillable(state)) => {
                    let Some(cell) = cell else { break };
                    self.ready = Some(Change {
                        position,
                        expected: cell.state,
                        state,
                        depositing: true,
                        drops: !cell.has_waterlogged_property && cell.fluid.is_none(),
                    });
                    break;
                }
                Some(Space::Filled) => {
                    self.queue.pop();
                    self.filled += 1;
                    if self.filled >= BLOCK_THRESHOLD {
                        self.infinite = true;
                        self.queue.clear();
                        self.queued.clear();
                        break;
                    }
                    for side in [0, 2, 3, 4, 5] {
                        let next = offset(position, FACES[side]);
                        let squared = distance_sq(self.root, next);
                        if squared <= RANGE * RANGE && self.queued.insert(next) {
                            self.queue.push(Reverse((
                                next.y,
                                distance + 1,
                                squared,
                                next.x,
                                next.z,
                            )));
                        }
                    }
                }
                Some(Space::Blocking) | None => {
                    self.queue.pop();
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Change {
    position: Pos,
    expected: u32,
    state: u32,
    depositing: bool,
    drops: bool,
}

impl Change {
    fn apply(self, world: World<'_>) -> bool {
        if !inspect(world, self.position).is_some_and(|cell| cell.state == self.expected) {
            return false;
        }
        let drops = self
            .drops
            .then(|| world.block_drops(self.position, None))
            .flatten();
        if world.write(&[BlockUpdate {
            position: self.position,
            state: self.state,
            flags: WORLD_FLAGS,
        }]) != 1
        {
            return false;
        }
        if self.depositing {
            if let Some(fluids) = fluids() {
                fluids.clear_ticks(world, self.position);
            }
            if let Some((stacks, drops)) = crate::STACKS.get().zip(drops) {
                for drop in drops {
                    stacks.spawn(
                        world,
                        [
                            f64::from(self.position.x) + 0.5,
                            f64::from(self.position.y) + 0.5,
                            f64::from(self.position.z) + 0.5,
                        ],
                        [0.0; 3],
                        &drop,
                    );
                }
            }
        } else {
            let above = offset(self.position, FACES[1]);
            if let Some(cell) = inspect(world, above)
                && cell.fluid.is_none()
                && world.can_survive(above, cell.state) == Some(false)
                && let Some(air) = CONTENT
                    .get()
                    .and_then(|content| content.state("minecraft:air"))
            {
                world.write(&[BlockUpdate {
                    position: above,
                    state: air,
                    flags: WORLD_FLAGS,
                }]);
            }
        }
        true
    }
}

#[derive(Clone, Copy)]
enum Deposit {
    Set(Change),
    Evaporate(Pos),
    Bottomless,
}

impl Deposit {
    fn apply(self, world: World<'_>) -> bool {
        match self {
            Self::Set(change) => change.apply(world),
            Self::Bottomless => true,
            Self::Evaporate(pos) => {
                if let Some(effects) = crate::EFFECTS.get() {
                    effects.play_sound(
                        world,
                        [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)],
                        "minecraft:block.fire.extinguish",
                        moddedmc_api::effects::SOUND_BLOCKS,
                        0.5,
                        2.6,
                    );
                }
                true
            }
        }
    }
}

fn has_block_state(id: &str) -> bool {
    matches!(id, "minecraft:water" | "minecraft:lava")
}

pub(super) struct HoseTransfer {
    position: Pos,
    before: TankInventory,
    pull: Option<Change>,
    deposit: Option<Deposit>,
}

impl HoseTransfer {
    pub fn source_fluid(world: World<'_>, pulley: &HosePulley) -> Option<FluidStack> {
        pulley.available(world).map(|(fluid, _, _)| fluid)
    }

    pub fn source(
        world: World<'_>,
        position: Pos,
        pulley: Option<&HosePulley>,
    ) -> Option<(Self, TankInventory)> {
        let before = load_tank(world, position)?;
        let mut available = before.clone();
        let mut pull = None;
        if before.amount() < BUCKET
            && let Some((fluid, source, infinite)) =
                pulley.and_then(|pulley| pulley.available(world))
            && before
                .fluid()
                .is_none_or(|stored| stored.same_fluid_same_components(&fluid))
        {
            if !infinite {
                let cell = inspect(world, source)?;
                let state = if cell.waterlogged {
                    waterlogged_state(&cell, false)?
                } else {
                    CONTENT.get()?.state("minecraft:air")?
                };
                pull = Some(Change {
                    position: source,
                    expected: cell.state,
                    state,
                    depositing: false,
                    drops: false,
                });
            }
            available.fill(&fluid, false);
        }
        Some((
            Self {
                position,
                before,
                pull,
                deposit: None,
            },
            available,
        ))
    }

    pub fn target(
        world: World<'_>,
        position: Pos,
        pulley: Option<&mut HosePulley>,
        fluid: &FluidStack,
    ) -> Option<(Self, TankInventory)> {
        if !has_block_state(&fluid.id) {
            return None;
        }
        let before = load_tank(world, position)?;
        let deposit = pulley.and_then(|pulley| {
            let root = pulley.root(position);
            if pulley.moving || root == position {
                return None;
            }
            if fluid.id == "minecraft:water" && fluids()?.water_evaporates(world, root) {
                return Some(Deposit::Evaporate(root));
            }
            if pulley
                .fill
                .as_ref()
                .is_none_or(|search| search.fluid != fluid.id)
            {
                pulley.fill = Some(FillSearch::new(world, root, fluid.id.clone()));
            }
            let search = pulley.fill.as_ref()?;
            if search.infinite && inspect(world, root)?.fluid.as_deref() == Some(fluid.id.as_str())
            {
                return Some(Deposit::Bottomless);
            }
            search.ready.map(Deposit::Set)
        });
        let capacity = CAPACITY + if deposit.is_some() { BUCKET } else { 0 };
        let mut tank = TankInventory::new(capacity.max(before.amount()), 1).ok()?;
        if let Some(stored) = before.fluid() {
            tank.fill(stored, false);
        }
        Some((
            Self {
                position,
                before,
                pull: None,
                deposit,
            },
            tank,
        ))
    }

    pub fn save(&self, world: World<'_>, tank: &TankInventory, pulleys: &mut HosePulleys) -> bool {
        let deposit = self.deposit.filter(|_| tank.amount() >= BUCKET);
        let mut stored = tank.clone();
        if deposit.is_some() {
            stored.drain_any(BUCKET, false);
        }
        let previous = pulleys
            .get(&self.position)
            .and_then(|pulley| pulley.affected);
        let change = self.pull.or(match deposit {
            Some(Deposit::Set(change)) => Some(change),
            _ => None,
        });
        let reset = previous
            .filter(|area| change.is_some_and(|change| area.depositing != change.depositing));
        let updated = change
            .map(|change| AffectedArea::include(previous.filter(|_| reset.is_none()), change));
        let Some(mut data) = smart_tank::read(world, self.position) else {
            return false;
        };
        data.insert("Tank".to_owned(), stored.content_nbt());
        if let Some(affected) = updated {
            affected.write(&mut data);
        }
        if !smart_tank::write(world, self.position, &data) {
            return false;
        }
        if self.pull.is_some_and(|change| !change.apply(world))
            || deposit.is_some_and(|change| !change.apply(world))
        {
            data.insert("Tank".to_owned(), self.before.content_nbt());
            data.remove("AffectedArea");
            if let Some(previous) = previous {
                previous.write(&mut data);
            }
            smart_tank::write(world, self.position, &data);
            return false;
        }
        if let Some(updated) = updated
            && let Some(pulley) = pulleys.blocks.get_mut(&self.position)
        {
            pulley.affected = Some(updated);
        }
        if let Some(reset) = reset {
            pulleys.reset(reset);
        }
        true
    }

    pub fn refund(&self, world: World<'_>, tank: &TankInventory) -> bool {
        save_tank(world, self.position, tank)
    }
}
