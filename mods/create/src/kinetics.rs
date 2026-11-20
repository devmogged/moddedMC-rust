//! Create 6.0.10's kinetic propagation and stress: `RotationPropagator`, `KineticNetwork`,
//! `KineticBlockEntity` and `GeneratingKineticBlockEntity`, ported method by method so the same
//! placements give the same speeds, sources, networks and broken blocks.

use std::collections::{HashMap, VecDeque};

use moddedmc_api::Pos;

use crate::belt_inventory::{BeltInventory, Chain};

/// `Direction.values()`: down, up, north, south, west, east.
pub const FACES: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
const DOWN: u8 = 0;
const NORTH: u8 = 2;

/// `CKinetics.maxRotationSpeed` default.
pub const MAX_ROTATION_SPEED: f32 = 256.0;
/// `CKinetics.kineticValidationFrequency` default.
const KINETIC_VALIDATION_FREQUENCY: i32 = 60;
/// `RotationPropagator.MAX_FLICKER_SCORE`.
const MAX_FLICKER_SCORE: i32 = 128;
/// The `limit` Create's belt chain walks use.
const MAX_CHAIN_WALK: usize = 1000;
/// `CreativeMotorBlockEntity.DEFAULT_SPEED`.
pub const DEFAULT_SPEED: i32 = 16;

pub const fn face_axis(face: u8) -> u8 {
    [1, 1, 2, 2, 0, 0][face as usize]
}

pub const fn positive(face: u8) -> bool {
    face % 2 == 1
}

pub const fn offset(pos: Pos, [x, y, z]: [i32; 3]) -> Pos {
    Pos {
        x: pos.x + x,
        y: pos.y + y,
        z: pos.z + z,
    }
}

const fn diff(from: Pos, to: Pos) -> [i32; 3] {
    [to.x - from.x, to.y - from.y, to.z - from.z]
}

/// `Direction.getNearest(int, int, int)`: the first direction with the largest positive dot
/// product, north for a zero vector.
fn nearest(diff: [i32; 3]) -> u8 {
    let mut best = NORTH;
    let mut score = 0;
    for (face, normal) in FACES.iter().enumerate() {
        let dot = diff[0] * normal[0] + diff[1] * normal[1] + diff[2] * normal[2];
        if dot > score {
            score = dot;
            best = face as u8;
        }
    }
    best
}

/// One kinetic block state, as the propagator sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Shaft(u8),
    Small(u8),
    Large(u8),
    /// Facing.
    Motor(u8),
    Gearbox(u8),
    /// Axis and `powered`.
    Clutch(u8, bool),
    Gearshift(u8, bool),
    EncasedShaft(u8),
    Turntable,
    Press(u8),
    Fan(u8),
    Millstone,
    Wheel(u8),
    Belt(Belt),
}

/// `BeltSlope`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slope {
    Horizontal,
    Upward,
    Downward,
    Vertical,
    Sideways,
}

impl Slope {
    pub const ALL: [Self; 5] = [
        Self::Horizontal,
        Self::Upward,
        Self::Downward,
        Self::Vertical,
        Self::Sideways,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Upward => "upward",
            Self::Downward => "downward",
            Self::Vertical => "vertical",
            Self::Sideways => "sideways",
        }
    }

    pub const fn diagonal(self) -> bool {
        matches!(self, Self::Upward | Self::Downward)
    }
}

/// `BeltPart`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    Start,
    Middle,
    End,
    Pulley,
}

impl Part {
    pub const ALL: [Self; 4] = [Self::Start, Self::Middle, Self::End, Self::Pulley];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Middle => "middle",
            Self::End => "end",
            Self::Pulley => "pulley",
        }
    }
}

/// A belt segment's `HORIZONTAL_FACING`, `SLOPE` and `PART`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Belt {
    pub facing: u8,
    pub slope: Slope,
    pub part: Part,
}

impl Belt {
    /// `BeltBlock.nextSegmentPosition`.
    pub const fn next_segment(self, pos: Pos, forward: bool) -> Option<Pos> {
        if matches!(
            (self.part, forward),
            (Part::End, true) | (Part::Start, false)
        ) {
            return None;
        }
        let offset = if forward { 1 } else { -1 };
        if matches!(self.slope, Slope::Vertical) {
            let up = if positive(self.facing) {
                offset
            } else {
                -offset
            };
            return Some(Pos {
                y: pos.y + up,
                ..pos
            });
        }
        let [x, _, z] = FACES[self.facing as usize];
        let pos = Pos {
            x: pos.x + x * offset,
            y: pos.y,
            z: pos.z + z * offset,
        };
        Some(match self.slope {
            Slope::Upward => Pos {
                y: pos.y + offset,
                ..pos
            },
            Slope::Downward => Pos {
                y: pos.y - offset,
                ..pos
            },
            _ => pos,
        })
    }

    /// `BeltBlockEntity.hasPulley`.
    pub const fn has_pulley(self) -> bool {
        !matches!(self.part, Part::Middle)
    }
}

impl Kind {
    /// `IRotate.getRotationAxis`.
    pub const fn axis(self) -> u8 {
        match self {
            Self::Shaft(axis)
            | Self::Small(axis)
            | Self::Large(axis)
            | Self::Gearbox(axis)
            | Self::Clutch(axis, _)
            | Self::Gearshift(axis, _)
            | Self::EncasedShaft(axis)
            | Self::Wheel(axis)
            | Self::Press(axis) => axis,
            Self::Motor(facing) | Self::Fan(facing) => face_axis(facing),
            Self::Turntable | Self::Millstone => 1,
            // BeltBlock.getRotationAxis: sideways belts turn on Y, others on the facing's
            // clockwise axis.
            Self::Belt(belt) => match belt.slope {
                Slope::Sideways => 1,
                _ if face_axis(belt.facing) == 2 => 0,
                _ => 2,
            },
        }
    }

    /// `IRotate.hasShaftTowards`.
    pub const fn has_shaft_towards(self, face: u8) -> bool {
        match self {
            Self::Motor(facing) => facing == face,
            Self::Fan(facing) => facing ^ 1 == face,
            Self::Gearbox(axis) => face_axis(face) != axis,
            Self::Turntable | Self::Millstone => face == DOWN,
            Self::Belt(belt) => face_axis(face) == self.axis() && belt.has_pulley(),
            _ => face_axis(face) == self.axis(),
        }
    }

    const fn small_cog(self) -> bool {
        matches!(self, Self::Small(_) | Self::Millstone)
    }

    const fn large_cog(self) -> bool {
        matches!(self, Self::Large(_))
    }

    /// `DirectionalShaftHalvesBlockEntity`: the blocks whose axis modifier depends on the source.
    const fn shaft_halves(self) -> bool {
        matches!(
            self,
            Self::Gearbox(_) | Self::Clutch(..) | Self::Gearshift(..)
        )
    }

    /// `CStress` impact at 1 RPM, from the `setImpact` calls in `AllBlocks`.
    pub const fn impact(self) -> f32 {
        match self {
            Self::Turntable | Self::Millstone => 4.0,
            Self::Press(_) | Self::Wheel(_) => 8.0,
            Self::Fan(_) => 2.0,
            _ => 0.0,
        }
    }

    /// `CStress` capacity at 1 RPM, from the `setCapacity` calls in `AllBlocks`.
    pub const fn capacity(self) -> f32 {
        match self {
            Self::Motor(_) => 16384.0,
            _ => 0.0,
        }
    }

    const fn generator(self) -> bool {
        matches!(self, Self::Motor(_))
    }

    fn same_block(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }

    /// `KineticBlock.areStatesKineticallyEquivalent`; belts also keep their `PART`.
    pub fn equivalent(self, other: Self) -> bool {
        let same_part = match (self, other) {
            (Self::Belt(a), Self::Belt(b)) => a.part == b.part,
            (Self::Fan(a), Self::Fan(b)) => a == b,
            _ => true,
        };
        self.same_block(other) && self.axis() == other.axis() && same_part
    }

    /// `canPropagateDiagonally`: small cogs and sloped belts.
    const fn propagates_diagonally(self) -> bool {
        match self {
            Self::Small(_) | Self::Millstone => true,
            Self::Belt(belt) => belt.slope.diagonal(),
            _ => false,
        }
    }

    /// `getPotentialNeighbourLocations` offsets: the six faces, then `addPropagationLocations`
    /// (small cogs: their in-plane diagonals; large cogs: every edge neighbour).
    fn neighbour_offsets(self) -> Vec<[i32; 3]> {
        let mut offsets = FACES.to_vec();
        if self.propagates_diagonally() || self.large_cog() {
            // BlockPos.betweenClosedStream order: x fastest, then y, then z.
            for z in -1..=1 {
                for y in -1..=1 {
                    for x in -1..=1 {
                        let step = [x, y, z];
                        if x * x + y * y + z * z != 2 {
                            continue;
                        }
                        if !self.large_cog() && step[usize::from(self.axis())] != 0 {
                            continue;
                        }
                        offsets.push(step);
                    }
                }
            }
        }
        offsets
    }
}

/// `BeltMovementHandler.TransportedEntityInfo`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Passenger {
    pub ticks_since_last_collision: i32,
    pub last_pos: Pos,
    pub last_state: u32,
}

impl Passenger {
    pub const fn new(collision: Pos, belt: u32) -> Self {
        Self {
            ticks_since_last_collision: 0,
            last_pos: collision,
            last_state: belt,
        }
    }
}

/// The saved and live fields of one `KineticBlockEntity`.
#[derive(Clone, Debug)]
pub struct Be {
    pub kind: Kind,
    serial: u64,
    pub speed: f32,
    pub source: Option<Pos>,
    pub network: Option<i64>,
    pub capacity: f32,
    pub stress: f32,
    pub network_size: i32,
    pub overstressed: bool,
    pub last_stress_applied: f32,
    pub last_capacity_provided: f32,
    /// The creative motor's scroll value.
    pub generated: i32,
    flicker_tally: i32,
    update_speed: bool,
    validation_countdown: i32,
    network_dirty: bool,
    re_activate_source: bool,
    initialized: bool,
    /// Changed since the last save: write the NBT and the rendered speed.
    pub dirty: bool,
    /// `BeltBlockEntity` chain fields; `controller` is `None` before `initBelt`.
    pub controller: Option<Pos>,
    pub belt_length: i32,
    pub belt_index: i32,
    /// A belt controller's entities by id, created on its first moving tick.
    pub passengers: Option<HashMap<i32, Passenger>>,
    /// A belt controller's items, created when the first one arrives.
    pub inventory: Option<BeltInventory>,
    pub press: Option<crate::press::PressingBehaviour>,
    pub fan: Option<Box<crate::fan::AirCurrent>>,
    pub millstone: Option<Box<crate::millstone::Millstone>>,
}

impl Be {
    /// A block entity as constructed, before its first tick.
    pub const fn new(kind: Kind) -> Self {
        Self {
            kind,
            serial: 0,
            speed: 0.0,
            source: None,
            network: None,
            capacity: 0.0,
            stress: 0.0,
            network_size: 0,
            overstressed: false,
            last_stress_applied: 0.0,
            last_capacity_provided: 0.0,
            generated: DEFAULT_SPEED,
            flicker_tally: 0,
            update_speed: true,
            validation_countdown: 0,
            network_dirty: false,
            re_activate_source: false,
            initialized: false,
            dirty: true,
            controller: None,
            belt_length: 0,
            belt_index: 0,
            inventory: None,
            passengers: None,
            fan: None,
            millstone: None,
            press: if matches!(kind, Kind::Press(_)) {
                Some(crate::press::PressingBehaviour::new(
                    crate::press::PressingData::EMPTY,
                ))
            } else {
                None
            },
        }
    }

    /// `BeltBlockEntity.getController`.
    pub fn controller_or(&self, pos: Pos) -> Pos {
        self.controller.unwrap_or(pos)
    }

    /// `CreativeMotorBlockEntity.getGeneratedSpeed`: `convertToDirection(value, facing)`.
    pub fn generated_speed(&self) -> f32 {
        match self.kind {
            Kind::Motor(facing) if positive(facing) => self.generated as f32,
            Kind::Motor(_) => -(self.generated as f32),
            _ => 0.0,
        }
    }

    fn is_source(&self) -> bool {
        self.generated_speed() != 0.0
    }

    /// `getSpeed`: zero while the network is overstressed.
    pub const fn get_speed(&self) -> f32 {
        if self.overstressed { 0.0 } else { self.speed }
    }

    fn calculate_stress_applied(&mut self) -> f32 {
        self.last_stress_applied = self.kind.impact();
        self.last_stress_applied
    }

    fn calculate_added_stress_capacity(&mut self) -> f32 {
        self.last_capacity_provided = self.kind.capacity();
        self.last_capacity_provided
    }

    fn on_speed_changed(&mut self, previous: f32) {
        let from_or_to_zero = (previous == 0.0) != (self.get_speed() == 0.0);
        let direction_swap = !from_or_to_zero && signum(previous) != signum(self.get_speed());
        if from_or_to_zero || direction_swap {
            self.flicker_tally += 5;
        }
        self.dirty = true;
    }

    /// `clearKineticInformation`.
    fn clear(&mut self) {
        self.speed = 0.0;
        self.source = None;
        self.network = None;
        self.overstressed = false;
        self.stress = 0.0;
        self.capacity = 0.0;
        self.last_stress_applied = 0.0;
        self.last_capacity_provided = 0.0;
        // BeltBlockEntity.clearKineticInformation
        if matches!(self.kind, Kind::Belt(_)) {
            self.belt_length = 0;
            self.belt_index = 0;
            self.controller = None;
        }
    }
}

/// Java's `Math.signum` for the comparisons the propagator makes.
const fn signum(value: f32) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

#[derive(Default)]
struct Network {
    initialized: bool,
    /// Position to (block entity serial, capacity or impact at 1 RPM).
    sources: HashMap<Pos, (u64, f32)>,
    members: HashMap<Pos, (u64, f32)>,
    current_capacity: f32,
    current_stress: f32,
    unloaded_capacity: f32,
    unloaded_stress: f32,
    unloaded_members: i32,
}

impl Network {
    fn size(&self) -> i32 {
        self.unloaded_members + self.members.len() as i32
    }
}

/// What the propagator needs from the world beyond the kinetic blocks it tracks.
pub trait Level {
    /// `Level.destroyBlock(pos, true)`.
    fn destroy(&mut self, pos: Pos);
    fn has_neighbor_signal(&self, pos: Pos) -> bool;
    fn is_loaded(&self, pos: Pos) -> bool;
    /// Replace the block state with `kind` without neighbor updates (clutch and gearshift
    /// `POWERED` toggles).
    fn set_kind(&mut self, pos: Pos, kind: Kind);
    /// `Level.setBlock(pos, state, UPDATE_ALL)` with `kind`'s default state (keeping water),
    /// or air.
    fn replace(&mut self, pos: Pos, kind: Option<Kind>);
}

/// The kinetic block entities and networks of one world.
#[derive(Default)]
pub struct Kinetics {
    pub depots: HashMap<Pos, crate::depot::DepotBehaviour>,
    pub chutes: crate::chute::Chutes,
    pub funnels: crate::funnel::Funnels,
    pub crushers: HashMap<Pos, crate::crusher::Controller>,
    pub recipes: Option<std::sync::Arc<crate::processing_recipes::RecipeBook>>,
    bes: HashMap<Pos, Be>,
    /// Tick order, `(position, serial)`, in insertion order like the level's ticker list.
    order: Vec<(Pos, u64)>,
    networks: HashMap<i64, Network>,
    next_serial: u64,
    /// Block ticks scheduled one tick ahead (gearshift and clutch re-attach).
    scheduled: Vec<Pos>,
    /// Belt inventories whose controller left since the last tick.
    dropped: Vec<Dropped>,
    pub dropped_millstones: Vec<(Pos, Box<crate::millstone::Millstone>)>,
}

/// A belt inventory whose controller was removed (`eject`, as `BeltBlockEntity.destroy`) or
/// unloaded.
pub struct Dropped {
    pub chain: Chain,
    pub inventory: BeltInventory,
    pub eject: bool,
}

enum Step {
    Continue,
    Return,
    /// Recurse into the position, then continue this loop.
    Descend(Pos),
    /// Recurse into the position, then return.
    Replace(Pos),
}

struct Frame {
    pos: Pos,
    serial: u64,
    neighbours: Vec<Pos>,
    index: usize,
}

/// `BlockPos.asLong`.
pub const fn packed(pos: Pos) -> i64 {
    ((pos.x as i64 & 0x3ff_ffff) << 38)
        | ((pos.z as i64 & 0x3ff_ffff) << 12)
        | (pos.y as i64 & 0xfff)
}

impl Kinetics {
    pub fn get(&self, pos: Pos) -> Option<&Be> {
        self.bes.get(&pos)
    }

    pub fn get_mut(&mut self, pos: Pos) -> Option<&mut Be> {
        self.bes.get_mut(&pos)
    }

    pub fn positions(&self, select: impl Fn(Kind) -> bool) -> Vec<Pos> {
        self.order
            .iter()
            .filter_map(|&(pos, serial)| {
                let be = self.bes.get(&pos)?;
                (be.serial == serial && select(be.kind)).then_some(pos)
            })
            .collect()
    }

    /// Belt controllers in ticker order.
    pub fn belt_controllers(&self) -> Vec<Pos> {
        self.order
            .iter()
            .filter(|&&(pos, serial)| {
                self.bes.get(&pos).is_some_and(|be| {
                    be.serial == serial
                        && matches!(be.kind, Kind::Belt(_))
                        && be.controller == Some(pos)
                })
            })
            .map(|&(pos, _)| pos)
            .collect()
    }

    pub fn kind(&self, pos: Pos) -> Option<Kind> {
        self.bes.get(&pos).map(|be| be.kind)
    }

    /// Changed block entities since the last call, for saving and rendering.
    pub fn drain_dirty(&mut self) -> Vec<(Pos, Be)> {
        let mut dirty = Vec::new();
        for (pos, be) in &mut self.bes {
            if be.dirty {
                be.dirty = false;
                dirty.push((*pos, be.clone()));
            }
        }
        dirty
    }

    /// A block entity created by placement or loaded with its saved fields.
    pub fn insert(&mut self, pos: Pos, mut be: Be) {
        if be.kind == Kind::Millstone && be.millstone.is_none() {
            be.millstone = Some(Box::default());
        }
        self.next_serial += 1;
        be.serial = self.next_serial;
        self.order.push((pos, be.serial));
        self.bes.insert(pos, be);
    }

    /// Chunk unload: the block entity leaves without `remove()`, as `SmartBlockEntity` skips it.
    pub fn unload(&mut self, pos: Pos) {
        if let Some(be) = self.bes.remove(&pos) {
            self.drop_inventory(pos, be, false);
        }
    }

    fn drop_inventory(&mut self, pos: Pos, mut be: Be, eject: bool) {
        if eject && let Some(millstone) = be.millstone.take() {
            self.dropped_millstones.push((pos, millstone));
        }
        let (Kind::Belt(belt), Some(inventory)) = (be.kind, be.inventory.take()) else {
            return;
        };
        let chain = Chain {
            pos,
            belt,
            length: be.belt_length,
            speed: be.get_speed(),
        };
        self.dropped.push(Dropped {
            chain,
            inventory,
            eject,
        });
    }

    pub fn take_dropped(&mut self) -> Vec<Dropped> {
        std::mem::take(&mut self.dropped)
    }

    /// `KineticBlockEntity.remove()` after its block is gone: leave the network, then
    /// `detachKinetics`.
    pub fn remove(&mut self, level: &mut dyn Level, pos: Pos) {
        let Some(be) = self.bes.remove(&pos) else {
            return;
        };
        if let Some(id) = be.network {
            self.network_remove(id, pos, be.serial, be.is_source());
        }
        self.handle_removed(level, pos, &be);
        let kind = be.kind;
        self.drop_inventory(pos, be, true);
        if let Kind::Belt(belt) = kind {
            self.remove_belt_neighbours(level, pos, belt);
        }
    }

    /// A state change that keeps the block entity (same block, same axis).
    pub fn set_kind(&mut self, pos: Pos, kind: Kind) {
        if let Some(be) = self.bes.get_mut(&pos) {
            be.kind = kind;
        }
    }

    /// `KineticBlock.updateIndirectNeighbourShapes` after the block changed axis in place.
    pub fn reset(&mut self, pos: Pos, kind: Kind) {
        if let Some(be) = self.bes.get_mut(&pos) {
            be.kind = kind;
            be.clear();
            be.update_speed = true;
            be.dirty = true;
        }
    }

    /// `GearshiftBlock.neighborChanged` and `ClutchBlock.neighborChanged`.
    pub fn neighbor_changed(&mut self, level: &mut dyn Level, pos: Pos) {
        let Some(kind) = self.kind(pos) else { return };
        let (axis, powered, clutch) = match kind {
            Kind::Clutch(axis, powered) => (axis, powered, true),
            Kind::Gearshift(axis, powered) => (axis, powered, false),
            _ => return,
        };
        if powered == level.has_neighbor_signal(pos) {
            return;
        }
        let cycled = if clutch {
            Kind::Clutch(axis, !powered)
        } else {
            Kind::Gearshift(axis, !powered)
        };
        if clutch {
            level.set_kind(pos, cycled);
            self.set_kind(pos, cycled);
            self.detach(level, pos, powered);
        } else {
            self.detach(level, pos, true);
            level.set_kind(pos, cycled);
            self.set_kind(pos, cycled);
        }
    }

    /// `GearshiftBlock.detachKinetics`.
    fn detach(&mut self, level: &mut dyn Level, pos: Pos, re_attach_next_tick: bool) {
        let Some(be) = self.bes.get(&pos).cloned() else {
            return;
        };
        self.handle_removed(level, pos, &be);
        if re_attach_next_tick {
            self.scheduled.push(pos);
        }
    }

    /// The creative motor's scroll value changed: `ScrollValueBehaviour` calls
    /// `updateGeneratedRotation`.
    pub fn set_generated(&mut self, level: &mut dyn Level, pos: Pos, rpm: i32) {
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        be.generated = rpm;
        be.dirty = true;
        self.update_generated_rotation(level, pos);
    }

    /// One server tick: due block ticks, then every block entity's `tick` in ticker order.
    pub fn tick(&mut self, level: &mut dyn Level) {
        for pos in std::mem::take(&mut self.scheduled) {
            // GearshiftBlock.tick
            if self.bes.contains_key(&pos) {
                self.handle_added(level, pos);
            }
        }
        let order = std::mem::take(&mut self.order);
        let mut kept = Vec::with_capacity(order.len());
        for (pos, serial) in order {
            if self.bes.get(&pos).is_some_and(|be| be.serial == serial) {
                kept.push((pos, serial));
                self.tick_be(level, pos, serial);
            }
        }
        // Block entities created during this tick were pushed onto the emptied list.
        kept.append(&mut self.order);
        self.order = kept;
    }

    fn alive(&self, pos: Pos, serial: u64) -> bool {
        self.bes.get(&pos).is_some_and(|be| be.serial == serial)
    }

    fn tick_be(&mut self, level: &mut dyn Level, pos: Pos, serial: u64) {
        // BeltBlockEntity.tick runs initBelt before KineticBlockEntity.tick.
        if matches!(self.bes[&pos].kind, Kind::Belt(_)) && self.bes[&pos].belt_length == 0 {
            self.init_belt(level, pos);
            if !self.alive(pos, serial) {
                return;
            }
        }
        // KineticBlockEntity.tick
        if self.bes[&pos].update_speed {
            self.attach(level, pos);
        }
        if !self.alive(pos, serial) {
            return;
        }
        // SmartBlockEntity.tick: initialize on the first tick.
        if !self.bes[&pos].initialized {
            if let Some(be) = self.bes.get_mut(&pos) {
                be.initialized = true;
            }
            self.initialize(level, pos);
            if !self.alive(pos, serial) {
                return;
            }
        }
        let be = self.bes.get_mut(&pos).expect("alive");
        be.validation_countdown -= 1;
        if be.validation_countdown < 0 {
            be.validation_countdown = KINETIC_VALIDATION_FREQUENCY;
            self.validate(level, pos);
            if !self.alive(pos, serial) {
                return;
            }
        }
        let be = self.bes.get_mut(&pos).expect("alive");
        if be.flicker_tally > 0 {
            be.flicker_tally -= 1;
        }
        if be.network_dirty {
            if let Some(id) = be.network {
                self.update_network(id);
            }
            if let Some(be) = self.bes.get_mut(&pos) {
                be.network_dirty = false;
            }
        }
        // GeneratingKineticBlockEntity.tick
        if self.bes.get(&pos).is_some_and(|be| be.re_activate_source) {
            self.update_generated_rotation(level, pos);
            if let Some(be) = self.bes.get_mut(&pos) {
                be.re_activate_source = false;
            }
        }
    }

    /// `KineticBlockEntity.initialize`, plus `CreativeMotorBlockEntity.initialize`.
    fn initialize(&mut self, level: &mut dyn Level, pos: Pos) {
        let be = &self.bes[&pos];
        if let Some(id) = be.network {
            let (capacity, stress, size) = (be.capacity, be.stress, be.network_size);
            let initialized = self.networks.get(&id).is_some_and(|n| n.initialized);
            if !initialized {
                self.init_from_te(id, capacity, stress, size);
            }
            self.add_silently(id, pos);
        }
        let be = &self.bes[&pos];
        if be.kind.generator() && (be.source.is_none() || be.generated_speed() > be.speed) {
            self.update_generated_rotation(level, pos);
        }
    }

    fn validate(&mut self, level: &mut dyn Level, pos: Pos) {
        let be = &self.bes[&pos];
        if let Some(source) = be.source {
            if be.network.is_none() {
                self.remove_source(pos);
                return;
            }
            if !level.is_loaded(source) {
                return;
            }
            if self
                .bes
                .get(&source)
                .is_none_or(|source| source.speed == 0.0)
            {
                self.remove_source(pos);
                self.detach_kinetics(level, pos);
            }
            return;
        }
        if be.speed != 0.0 && be.generated_speed() == 0.0 {
            self.bes.get_mut(&pos).expect("validated").speed = 0.0;
        }
    }

    fn attach(&mut self, level: &mut dyn Level, pos: Pos) {
        if let Some(be) = self.bes.get_mut(&pos) {
            be.update_speed = false;
        }
        self.handle_added(level, pos);
    }

    fn detach_kinetics(&mut self, level: &mut dyn Level, pos: Pos) {
        if let Some(be) = self.bes.get(&pos).cloned() {
            self.handle_removed(level, pos, &be);
        }
    }

    // RotationPropagator

    /// `getRotationSpeedModifier`.
    fn rotation_speed_modifier(&self, from: Pos, to: Pos) -> f32 {
        let (Some(from_be), Some(to_be)) = (self.bes.get(&from), self.bes.get(&to)) else {
            return 0.0;
        };
        let (state_from, state_to) = (from_be.kind, to_be.kind);
        let diff = diff(from, to);
        let direction = nearest(diff);
        let direction_axis = usize::from(face_axis(direction));
        let aligned_axes = (0..3).all(|axis| axis == direction_axis || diff[axis] == 0);
        let connected_by_axis = aligned_axes
            && state_from.has_shaft_towards(direction)
            && state_to.has_shaft_towards(direction ^ 1);
        let connected_by_gears = state_from.small_cog() && state_to.small_cog();

        // BeltBlockEntity.propagateRotationTo: segments of one belt turn together.
        if let (Kind::Belt(_), Kind::Belt(_)) = (state_from, state_to)
            && !connected_by_axis
            && from_be.controller_or(from) == to_be.controller_or(to)
        {
            return 1.0;
        }

        if connected_by_axis {
            let mut axis_modifier = Self::axis_modifier(to, to_be, direction ^ 1);
            if axis_modifier != 0.0 {
                axis_modifier = 1.0 / axis_modifier;
            }
            return Self::axis_modifier(from, from_be, direction) * axis_modifier;
        }

        if large_to_large(state_from, state_to, diff) {
            let source_axis_diff = diff[usize::from(state_from.axis())];
            let target_axis_diff = diff[usize::from(state_to.axis())];
            return if (source_axis_diff > 0) ^ (target_axis_diff > 0) {
                -1.0
            } else {
                1.0
            };
        }

        if state_from.large_cog()
            && state_to.small_cog()
            && large_to_small(state_from, state_to, diff)
        {
            return -2.0;
        }
        if state_to.large_cog()
            && state_from.small_cog()
            && large_to_small(state_to, state_from, diff)
        {
            return -0.5;
        }

        if connected_by_gears {
            if diff.iter().map(|d| d.abs()).sum::<i32>() != 1 {
                return 0.0;
            }
            if state_to.large_cog() {
                return 0.0;
            }
            if face_axis(direction) == state_from.axis() {
                return 0.0;
            }
            if state_from.axis() == state_to.axis() {
                return -1.0;
            }
        }
        0.0
    }

    /// `getAxisModifier`: gearboxes and split shafts turn some faces against their source.
    fn axis_modifier(pos: Pos, be: &Be, direction: u8) -> f32 {
        if !(be.source.is_some() || be.is_source()) || !be.kind.shaft_halves() {
            return 1.0;
        }
        let Some(source) = be.source else {
            return 1.0;
        };
        // DirectionalShaftHalvesBlockEntity.getSourceFacing
        let source = nearest(diff(pos, source));
        match be.kind {
            Kind::Gearbox(_) => {
                if face_axis(direction) == face_axis(source) {
                    if direction == source { 1.0 } else { -1.0 }
                } else if positive(direction) == positive(source) {
                    -1.0
                } else {
                    1.0
                }
            }
            // ClutchBlockEntity and GearshiftBlockEntity.getRotationSpeedModifier
            Kind::Clutch(_, true) if direction != source => 0.0,
            Kind::Gearshift(_, true) if direction != source => -1.0,
            _ => 1.0,
        }
    }

    /// `getConveyedSpeed`.
    fn conveyed_speed(&self, from: Pos, to: Pos) -> f32 {
        self.bes[&from].speed * self.rotation_speed_modifier(from, to)
    }

    fn is_connected(&self, from: Pos, to: Pos) -> bool {
        self.rotation_speed_modifier(from, to) != 0.0
    }

    /// `getPotentialNeighbourLocations` for a block of `kind` at `pos`.
    fn potential_neighbours(kind: Kind, pos: Pos) -> impl Iterator<Item = Pos> {
        kind.neighbour_offsets()
            .into_iter()
            .map(move |step| offset(pos, step))
    }

    /// `getConnectedNeighbours`.
    fn connected_neighbours(&self, pos: Pos) -> Vec<Pos> {
        let Some(be) = self.bes.get(&pos) else {
            return Vec::new();
        };
        Self::potential_neighbours(be.kind, pos)
            .filter(|&neighbour| {
                self.bes.contains_key(&neighbour)
                    && (self.is_connected(pos, neighbour) || self.is_connected(neighbour, pos))
            })
            .collect()
    }

    fn handle_added(&mut self, level: &mut dyn Level, pos: Pos) {
        if self.bes.contains_key(&pos) {
            self.propagate_new_source(level, pos);
        }
    }

    fn frame(&self, pos: Pos) -> Option<Frame> {
        Some(Frame {
            pos,
            serial: self.bes.get(&pos)?.serial,
            neighbours: self.connected_neighbours(pos),
            index: 0,
        })
    }

    /// `propagateNewSource`. Create recurses; this keeps the same visiting order on an explicit
    /// stack so long lines can't overflow the server thread.
    fn propagate_new_source(&mut self, level: &mut dyn Level, start: Pos) {
        let mut stack: Vec<Frame> = self.frame(start).into_iter().collect();
        while let Some(frame) = stack.last_mut() {
            let (current, serial) = (frame.pos, frame.serial);
            let Some(&neighbour) = frame.neighbours.get(frame.index) else {
                stack.pop();
                continue;
            };
            frame.index += 1;
            // A block destroyed deeper in the walk ends its own loop.
            if !self.alive(current, serial) {
                stack.pop();
                continue;
            }
            if !self.bes.contains_key(&neighbour) {
                continue;
            }
            match self.propagate_step(level, current, neighbour) {
                Step::Continue => {}
                Step::Return => {
                    stack.pop();
                }
                Step::Descend(next) => stack.extend(self.frame(next)),
                Step::Replace(next) => {
                    stack.pop();
                    stack.extend(self.frame(next));
                }
            }
        }
    }

    fn propagate_step(&mut self, level: &mut dyn Level, current: Pos, neighbour: Pos) -> Step {
        let speed_of_current = self.bes[&current].speed;
        let speed_of_neighbour = self.bes[&neighbour].speed;
        let new_speed = self.conveyed_speed(current, neighbour);
        let opposite_speed = self.conveyed_speed(neighbour, current);

        if new_speed == 0.0 && opposite_speed == 0.0 {
            return Step::Continue;
        }

        let incompatible = signum(new_speed) != signum(speed_of_neighbour)
            && (new_speed != 0.0 && speed_of_neighbour != 0.0);

        let too_fast =
            new_speed.abs() > MAX_ROTATION_SPEED || opposite_speed.abs() > MAX_ROTATION_SPEED;
        let speed_changed_too_often = self.bes[&current].flicker_tally > MAX_FLICKER_SCORE;
        if too_fast || speed_changed_too_often || incompatible {
            self.destroy(level, current);
            return Step::Return;
        }

        // Neighbour faster, overpower the incoming tree
        if opposite_speed.abs() > speed_of_current.abs() {
            let previous = self.bes[&current].get_speed();
            self.set_source(current, neighbour);
            let speed = self.conveyed_speed(neighbour, current);
            if let Some(be) = self.bes.get_mut(&current) {
                be.speed = speed;
                be.on_speed_changed(previous);
            }
            return Step::Replace(current);
        }

        // Current faster, overpower the neighbours' tree
        if new_speed.abs() >= speed_of_neighbour.abs() {
            // Do not overpower your own network -> cycle
            let (current_network, neighbour_network) =
                (self.bes[&current].network, self.bes[&neighbour].network);
            if current_network.is_none() || current_network == neighbour_network {
                let epsilon = speed_of_neighbour.abs() / 256.0 / 256.0;
                if new_speed.abs() > speed_of_neighbour.abs() + epsilon {
                    // Create keeps looping with the removed block entity; nothing it does there
                    // survives the next validation, so the walk stops here.
                    self.destroy(level, current);
                    return Step::Return;
                }
                return Step::Continue;
            }

            if self.bes[&current].source == Some(neighbour) {
                self.remove_source(current);
            }

            let previous = self.bes[&neighbour].get_speed();
            self.set_source(neighbour, current);
            let speed = self.conveyed_speed(current, neighbour);
            if let Some(be) = self.bes.get_mut(&neighbour) {
                be.speed = speed;
                be.on_speed_changed(previous);
            }
            return Step::Descend(neighbour);
        }

        if (self.bes[&neighbour].speed - new_speed).abs() <= 1e-4 {
            return Step::Continue;
        }

        let previous = self.bes[&neighbour].get_speed();
        self.bes.get_mut(&neighbour).expect("neighbour").speed = new_speed;
        self.set_source(neighbour, current);
        if let Some(be) = self.bes.get_mut(&neighbour) {
            be.on_speed_changed(previous);
        }
        Step::Descend(neighbour)
    }

    /// `handleRemoved`, with the removed block entity's last fields.
    fn handle_removed(&mut self, level: &mut dyn Level, pos: Pos, removed: &Be) {
        if removed.speed == 0.0 {
            return;
        }
        for neighbour in Self::potential_neighbours(removed.kind, pos) {
            if self
                .bes
                .get(&neighbour)
                .is_some_and(|be| be.source == Some(pos))
            {
                self.propagate_missing_source(level, neighbour);
            }
        }
    }

    /// `propagateMissingSource`.
    fn propagate_missing_source(&mut self, level: &mut dyn Level, start: Pos) {
        let mut potential_new_sources = Vec::new();
        let mut frontier = VecDeque::from([start]);
        let missing_source = self.bes.get(&start).and_then(|be| be.source);

        while let Some(pos) = frontier.pop_front() {
            if !self.bes.contains_key(&pos) {
                continue;
            }
            self.remove_source(pos);

            for neighbour in self.connected_neighbours(pos) {
                if Some(neighbour) == missing_source {
                    continue;
                }
                let be = &self.bes[&neighbour];
                let Some(source) = be.source else {
                    continue;
                };
                if source != pos {
                    potential_new_sources.push(neighbour);
                    continue;
                }
                if be.is_source() {
                    potential_new_sources.push(neighbour);
                }
                frontier.push_back(neighbour);
            }
        }

        for new_source in potential_new_sources {
            if self
                .bes
                .get(&new_source)
                .is_some_and(|be| be.source.is_some() || be.is_source())
            {
                self.propagate_new_source(level, new_source);
                return;
            }
        }
    }

    /// `BeltBlock.getBeltChain` from the controller.
    pub fn belt_chain(&self, controller: Pos) -> Vec<Pos> {
        let mut chain = Vec::new();
        let mut current = Some(controller);
        for _ in 0..MAX_CHAIN_WALK {
            let Some(pos) = current else { break };
            let Some(Kind::Belt(belt)) = self.kind(pos) else {
                break;
            };
            chain.push(pos);
            current = belt.next_segment(pos, true);
        }
        chain
    }

    /// `BeltBlock.initBelt`: find the controller walking backwards, then number the chain.
    fn init_belt(&mut self, level: &mut dyn Level, pos: Pos) {
        let mut current = pos;
        for _ in 0..MAX_CHAIN_WALK {
            let Some(Kind::Belt(belt)) = self.kind(current) else {
                self.destroy(level, pos);
                return;
            };
            let Some(next) = belt.next_segment(current, false) else {
                break;
            };
            if !level.is_loaded(next) {
                return;
            }
            current = next;
        }
        let chain = self.belt_chain(current);
        if chain.len() < 2 {
            self.destroy(level, current);
            return;
        }
        let length = chain.len() as i32;
        for (index, belt) in chain.into_iter().enumerate() {
            let Some(be) = self.bes.get_mut(&belt) else {
                self.destroy(level, current);
                return;
            };
            be.controller = Some(current);
            be.belt_length = length;
            be.belt_index = index as i32;
            be.dirty = true;
            self.attach(level, belt);
        }
    }

    /// `BeltBlock.onRemove`: the segments on either side turn back into shafts (pulleys) or
    /// air, which removes the rest of the chain the same way.
    fn remove_belt_neighbours(&mut self, level: &mut dyn Level, pos: Pos, belt: Belt) {
        for forward in [true, false] {
            let Some(next) = belt.next_segment(pos, forward) else {
                continue;
            };
            let Some(Kind::Belt(next_belt)) = self.kind(next) else {
                continue;
            };
            let shaft = next_belt
                .has_pulley()
                .then(|| Kind::Shaft(Kind::Belt(next_belt).axis()));
            level.replace(next, shaft);
            self.remove(level, next);
        }
    }

    /// `Level.destroyBlock(pos, true)`: the block entity's `remove()` runs inside it.
    fn destroy(&mut self, level: &mut dyn Level, pos: Pos) {
        level.destroy(pos);
        self.remove(level, pos);
    }

    // KineticBlockEntity source and network bookkeeping

    fn set_source(&mut self, pos: Pos, source: Pos) {
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        be.source = Some(source);
        let Some(source_be) = self.bes.get(&source) else {
            self.remove_source(pos);
            return;
        };
        let (network, source_speed) = (source_be.network, source_be.get_speed());
        self.set_network(pos, network);
        // GeneratingKineticBlockEntity.setSource
        if let Some(be) = self.bes.get_mut(&pos)
            && be.re_activate_source
            && source_speed.abs() >= be.generated_speed().abs()
        {
            be.re_activate_source = false;
        }
    }

    fn remove_source(&mut self, pos: Pos) {
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        // GeneratingKineticBlockEntity.removeSource
        if be.source.is_some() && be.is_source() {
            be.re_activate_source = true;
        }
        let previous = be.get_speed();
        be.speed = 0.0;
        be.source = None;
        self.set_network(pos, None);
        if let Some(be) = self.bes.get_mut(&pos) {
            be.on_speed_changed(previous);
        }
    }

    fn set_network(&mut self, pos: Pos, network: Option<i64>) {
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        if be.network == network {
            return;
        }
        let (old, serial, is_source) = (be.network, be.serial, be.is_source());
        if let Some(old) = old {
            self.network_remove(old, pos, serial, is_source);
        }
        let be = self.bes.get_mut(&pos).expect("still present");
        be.network = network;
        be.dirty = true;
        let Some(id) = network else { return };
        self.networks.entry(id).or_default().initialized = true;
        self.network_add(id, pos);
    }

    // KineticNetwork

    fn init_from_te(&mut self, id: i64, max_stress: f32, current_stress: f32, members: i32) {
        let network = self.networks.entry(id).or_default();
        network.unloaded_capacity = max_stress;
        network.unloaded_stress = current_stress;
        network.unloaded_members = members;
        network.initialized = true;
        self.update_stress(id);
        self.update_capacity(id);
    }

    fn add_silently(&mut self, id: i64, pos: Pos) {
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        let network = self.networks.entry(id).or_default();
        if network
            .members
            .get(&pos)
            .is_some_and(|(serial, _)| *serial == be.serial)
        {
            return;
        }
        if be.is_source() {
            network.unloaded_capacity -= be.last_capacity_provided * be.generated_speed().abs();
            let capacity = be.calculate_added_stress_capacity();
            network.sources.insert(pos, (be.serial, capacity));
        }
        network.unloaded_stress -= be.last_stress_applied * be.speed.abs();
        let stress = be.calculate_stress_applied();
        network.members.insert(pos, (be.serial, stress));
        network.unloaded_members = (network.unloaded_members - 1).max(0);
        network.unloaded_capacity = network.unloaded_capacity.max(0.0);
        network.unloaded_stress = network.unloaded_stress.max(0.0);
    }

    fn network_add(&mut self, id: i64, pos: Pos) {
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        let network = self.networks.entry(id).or_default();
        if network
            .members
            .get(&pos)
            .is_some_and(|(serial, _)| *serial == be.serial)
        {
            return;
        }
        if be.is_source() {
            let capacity = be.calculate_added_stress_capacity();
            network.sources.insert(pos, (be.serial, capacity));
        }
        let stress = be.calculate_stress_applied();
        network.members.insert(pos, (be.serial, stress));
        let (capacity, stress, size) = (
            network.current_capacity,
            network.current_stress,
            network.size(),
        );
        self.update_from_network(pos, capacity, stress, size);
        if let Some(be) = self.bes.get_mut(&pos) {
            be.network_dirty = true;
        }
    }

    fn network_remove(&mut self, id: i64, pos: Pos, serial: u64, is_source: bool) {
        let Some(network) = self.networks.get_mut(&id) else {
            return;
        };
        if network
            .members
            .get(&pos)
            .is_none_or(|(member, _)| *member != serial)
        {
            return;
        }
        if is_source {
            network.sources.remove(&pos);
        }
        network.members.remove(&pos);
        let first = network.members.keys().next().copied();
        if self.alive(pos, serial) {
            self.update_from_network(pos, 0.0, 0.0, 0);
        }
        match first {
            None => {
                self.networks.remove(&id);
            }
            Some(first) => {
                if let Some(be) = self.bes.get_mut(&first) {
                    be.network_dirty = true;
                }
            }
        }
    }

    fn update_capacity_for(&mut self, id: i64, pos: Pos, capacity: f32) {
        let serial = self.bes[&pos].serial;
        self.networks
            .entry(id)
            .or_default()
            .sources
            .insert(pos, (serial, capacity));
        self.update_capacity(id);
    }

    fn update_stress_for(&mut self, id: i64, pos: Pos, stress: f32) {
        let serial = self.bes[&pos].serial;
        self.networks
            .entry(id)
            .or_default()
            .members
            .insert(pos, (serial, stress));
        self.update_stress(id);
    }

    fn sync(&mut self, id: i64) {
        let Some(network) = self.networks.get(&id) else {
            return;
        };
        let (capacity, stress, size) = (
            network.current_capacity,
            network.current_stress,
            network.size(),
        );
        let members: Vec<_> = network.members.keys().copied().collect();
        for pos in members {
            self.update_from_network(pos, capacity, stress, size);
        }
    }

    fn update_capacity(&mut self, id: i64) {
        let capacity = self.calculate_capacity(id);
        let network = self.networks.entry(id).or_default();
        if network.current_capacity != capacity {
            network.current_capacity = capacity;
            self.sync(id);
        }
    }

    fn update_stress(&mut self, id: i64) {
        let stress = self.calculate_stress(id);
        let network = self.networks.entry(id).or_default();
        if network.current_stress != stress {
            network.current_stress = stress;
            self.sync(id);
        }
    }

    fn update_network(&mut self, id: i64) {
        let stress = self.calculate_stress(id);
        let capacity = self.calculate_capacity(id);
        let network = self.networks.entry(id).or_default();
        if network.current_stress != stress || network.current_capacity != capacity {
            network.current_stress = stress;
            network.current_capacity = capacity;
            self.sync(id);
        }
    }

    fn calculate_capacity(&mut self, id: i64) -> f32 {
        let bes = &self.bes;
        let network = self.networks.entry(id).or_default();
        network
            .sources
            .retain(|pos, (serial, _)| bes.get(pos).is_some_and(|be| be.serial == *serial));
        let present: f32 = network
            .sources
            .iter()
            .map(|(pos, (_, capacity))| capacity * bes[pos].generated_speed().abs())
            .sum();
        present + network.unloaded_capacity
    }

    fn calculate_stress(&mut self, id: i64) -> f32 {
        let bes = &self.bes;
        let network = self.networks.entry(id).or_default();
        network
            .members
            .retain(|pos, (serial, _)| bes.get(pos).is_some_and(|be| be.serial == *serial));
        let present: f32 = network
            .members
            .iter()
            .map(|(pos, (_, impact))| impact * bes[pos].speed.abs())
            .sum();
        present + network.unloaded_stress
    }

    /// `KineticBlockEntity.updateFromNetwork`.
    fn update_from_network(&mut self, pos: Pos, max_stress: f32, current_stress: f32, size: i32) {
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        be.network_dirty = false;
        be.capacity = max_stress;
        be.stress = current_stress;
        be.network_size = size;
        let overstressed = max_stress < current_stress;
        be.dirty = true;
        if overstressed != be.overstressed {
            let previous = be.get_speed();
            be.overstressed = overstressed;
            be.on_speed_changed(previous);
        }
    }

    // GeneratingKineticBlockEntity

    fn update_generated_rotation(&mut self, level: &mut dyn Level, pos: Pos) {
        let Some(be) = self.bes.get(&pos) else {
            return;
        };
        let speed = be.generated_speed();
        let previous = be.speed;
        if previous != speed {
            self.apply_new_speed(level, pos, previous, speed);
        }
        let Some(be) = self.bes.get_mut(&pos) else {
            return;
        };
        if let Some(id) = be.network
            && speed != 0.0
        {
            let capacity = be.calculate_added_stress_capacity();
            let impact = be.calculate_stress_applied();
            self.update_capacity_for(id, pos, capacity);
            self.update_stress_for(id, pos, impact);
            self.update_stress(id);
        }
        if let Some(be) = self.bes.get_mut(&pos) {
            be.on_speed_changed(previous);
        }
    }

    fn apply_new_speed(&mut self, level: &mut dyn Level, pos: Pos, previous: f32, speed: f32) {
        let be = &self.bes[&pos];
        let (source, network) = (be.source, be.network);
        // Speed changed to 0
        if speed == 0.0 {
            if source.is_some() {
                if let Some(id) = network {
                    self.update_capacity_for(id, pos, 0.0);
                    let impact = self
                        .bes
                        .get_mut(&pos)
                        .expect("generator")
                        .calculate_stress_applied();
                    self.update_stress_for(id, pos, impact);
                }
                return;
            }
            self.detach_kinetics(level, pos);
            self.bes.get_mut(&pos).expect("generator").speed = 0.0;
            self.set_network(pos, None);
            return;
        }
        // Now turning - create a new Network
        if previous == 0.0 {
            self.bes.get_mut(&pos).expect("generator").speed = speed;
            self.set_network(pos, Some(packed(pos)));
            self.attach(level, pos);
            return;
        }
        // Change speed when overpowered by other generator
        if source.is_some() {
            // Staying below Overpowered speed
            if previous.abs() >= speed.abs() {
                if signum(previous) != signum(speed) {
                    self.destroy(level, pos);
                }
                return;
            }
            // Faster than attached network -> become the new source
            self.detach_kinetics(level, pos);
            let be = self.bes.get_mut(&pos).expect("generator");
            be.speed = speed;
            be.source = None;
            self.set_network(pos, Some(packed(pos)));
            self.attach(level, pos);
            return;
        }
        // Reapply source
        self.detach_kinetics(level, pos);
        self.bes.get_mut(&pos).expect("generator").speed = speed;
        self.attach(level, pos);
    }
}

/// `isLargeToLargeGear`.
fn large_to_large(from: Kind, to: Kind, diff: [i32; 3]) -> bool {
    if !from.large_cog() || !to.large_cog() || from.axis() == to.axis() {
        return false;
    }
    (0..3u8).all(|axis| {
        let axis_diff = diff[usize::from(axis)];
        if axis == from.axis() || axis == to.axis() {
            axis_diff != 0
        } else {
            axis_diff == 0
        }
    })
}

/// `isLargeToSmallCog`.
fn large_to_small(from: Kind, to: Kind, diff: [i32; 3]) -> bool {
    let axis_from = from.axis();
    if axis_from != to.axis() || diff[usize::from(axis_from)] != 0 {
        return false;
    }
    (0..3u8)
        .filter(|&axis| axis != axis_from)
        .all(|axis| diff[usize::from(axis)].abs() == 1)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Be, Belt, Kind, Kinetics, Level, Part, Pos, Slope};

    const UP: u8 = 1;
    const WEST: u8 = 4;
    const EAST: u8 = 5;
    const X: u8 = 0;
    const Y: u8 = 1;
    const Z: u8 = 2;

    const fn pos(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }

    #[derive(Default)]
    struct TestLevel {
        destroyed: Vec<Pos>,
        replaced: Vec<Pos>,
        signal: HashSet<Pos>,
    }

    impl Level for TestLevel {
        fn destroy(&mut self, pos: Pos) {
            self.destroyed.push(pos);
        }
        fn has_neighbor_signal(&self, pos: Pos) -> bool {
            self.signal.contains(&pos)
        }
        fn is_loaded(&self, _pos: Pos) -> bool {
            true
        }
        fn set_kind(&mut self, _pos: Pos, _kind: Kind) {}
        fn replace(&mut self, pos: Pos, _kind: Option<Kind>) {
            self.replaced.push(pos);
        }
    }

    struct Rig {
        kinetics: Kinetics,
        level: TestLevel,
    }

    impl Rig {
        fn new() -> Self {
            Self {
                kinetics: Kinetics::default(),
                level: TestLevel::default(),
            }
        }

        fn motor(&mut self, at: Pos, rpm: i32, facing: u8) {
            let mut be = Be::new(Kind::Motor(facing));
            be.generated = rpm;
            self.kinetics.insert(at, be);
        }

        fn place(&mut self, blocks: &[(Pos, Kind)]) {
            for &(at, kind) in blocks {
                self.kinetics.insert(at, Be::new(kind));
            }
        }

        fn ticks(&mut self, count: usize) {
            for _ in 0..count {
                self.kinetics.tick(&mut self.level);
            }
        }

        fn speed(&self, at: Pos) -> f32 {
            self.kinetics.get(at).map_or(f32::NAN, Be::get_speed)
        }
    }

    /// P1a's rig: a motor facing up at 16 RPM, a shaft, a large cog and two meshed small cogs.
    /// The large cog doubles into a small cog (-2), which reverses into the next (-1).
    #[test]
    fn motor_shaft_and_cogs_take_create_ratios() {
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 16, UP);
        rig.place(&[
            (pos(0, 1, 0), Kind::Shaft(Y)),
            (pos(0, 2, 0), Kind::Large(Y)),
            (pos(1, 2, 1), Kind::Small(Y)),
            (pos(2, 2, 1), Kind::Small(Y)),
        ]);
        rig.ticks(3);
        assert_eq!(rig.speed(pos(0, 1, 0)), 16.0);
        assert_eq!(rig.speed(pos(0, 2, 0)), 16.0);
        assert_eq!(rig.speed(pos(1, 2, 1)), -32.0);
        assert_eq!(rig.speed(pos(2, 2, 1)), 32.0);
        assert_eq!(
            rig.kinetics.get(pos(1, 2, 1)).and_then(|be| be.source),
            Some(pos(0, 2, 0))
        );
    }

    /// A gearbox fed from the west: the straight-through east shaft reverses, and of the side
    /// shafts the north one (negative, like the west source) reverses too.
    #[test]
    fn gearbox_turns_faces_against_their_source() {
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 16, EAST);
        rig.place(&[
            (pos(1, 0, 0), Kind::Gearbox(Y)),
            (pos(2, 0, 0), Kind::Shaft(X)),
            (pos(1, 0, -1), Kind::Shaft(Z)),
            (pos(1, 0, 1), Kind::Shaft(Z)),
        ]);
        rig.ticks(3);
        assert_eq!(rig.speed(pos(1, 0, 0)), 16.0);
        assert_eq!(rig.speed(pos(2, 0, 0)), -16.0);
        assert_eq!(rig.speed(pos(1, 0, -1)), -16.0);
        assert_eq!(rig.speed(pos(1, 0, 1)), 16.0);
    }

    #[test]
    fn powered_gearshift_reverses_and_powered_clutch_disconnects() {
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 16, EAST);
        rig.place(&[
            (pos(1, 0, 0), Kind::Gearshift(X, false)),
            (pos(2, 0, 0), Kind::Shaft(X)),
            (pos(3, 0, 0), Kind::Clutch(X, false)),
            (pos(4, 0, 0), Kind::Shaft(X)),
        ]);
        rig.ticks(3);
        assert_eq!(rig.speed(pos(2, 0, 0)), 16.0);
        assert_eq!(rig.speed(pos(4, 0, 0)), 16.0);

        rig.level.signal.insert(pos(1, 0, 0));
        rig.kinetics.neighbor_changed(&mut rig.level, pos(1, 0, 0));
        rig.ticks(2);
        assert_eq!(
            rig.kinetics.kind(pos(1, 0, 0)),
            Some(Kind::Gearshift(X, true))
        );
        assert_eq!(rig.speed(pos(1, 0, 0)), 16.0);
        assert_eq!(rig.speed(pos(2, 0, 0)), -16.0);
        assert_eq!(rig.speed(pos(4, 0, 0)), -16.0);

        rig.level.signal.insert(pos(3, 0, 0));
        rig.kinetics.neighbor_changed(&mut rig.level, pos(3, 0, 0));
        rig.ticks(2);
        assert_eq!(rig.speed(pos(3, 0, 0)), -16.0);
        assert_eq!(rig.speed(pos(4, 0, 0)), 0.0);

        rig.level.signal.remove(&pos(3, 0, 0));
        rig.kinetics.neighbor_changed(&mut rig.level, pos(3, 0, 0));
        rig.ticks(2);
        assert_eq!(rig.speed(pos(4, 0, 0)), -16.0);
        assert!(rig.level.destroyed.is_empty());
    }

    /// A motor at 1 RPM gives 16384 SU. Eight large-to-small steps (x-2 each) reach 256 RPM,
    /// where a turntable (impact 4) costs 1024 SU: sixteen fit exactly, seventeen overstress.
    fn turntable_rig(turntables: i32) -> Rig {
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 1, UP);
        for step in 0..8 {
            rig.place(&[
                (pos(step, step + 1, step), Kind::Large(Y)),
                (pos(step + 1, step + 1, step + 1), Kind::Small(Y)),
            ]);
        }
        for column in 1..turntables {
            rig.place(&[(pos(8 + column, 8, 8), Kind::Small(Y))]);
        }
        for column in 0..turntables {
            rig.place(&[(pos(8 + column, 9, 8), Kind::Turntable)]);
        }
        rig.ticks(4);
        rig
    }

    #[test]
    fn network_overstresses_past_its_capacity_and_recovers() {
        let fitting = turntable_rig(16);
        let be = fitting.kinetics.get(pos(8, 9, 8)).expect("turntable");
        assert_eq!(be.speed, 256.0);
        assert_eq!((be.capacity, be.stress), (16384.0, 16384.0));
        assert!(!be.overstressed);

        let mut rig = turntable_rig(17);
        let be = rig.kinetics.get(pos(8, 9, 8)).expect("turntable");
        assert_eq!((be.capacity, be.stress), (16384.0, 17408.0));
        assert_eq!(be.network_size, 1 + 8 + 8 + 16 + 17);
        for probe in [pos(0, 0, 0), pos(3, 4, 3), pos(8, 9, 8), pos(24, 9, 8)] {
            let be = rig.kinetics.get(probe).expect("member");
            assert!(be.overstressed, "{probe:?} should be overstressed");
            assert_eq!(be.get_speed(), 0.0);
            assert_ne!(be.speed, 0.0, "{probe:?} keeps its theoretical speed");
        }

        rig.kinetics.remove(&mut rig.level, pos(24, 9, 8));
        rig.ticks(2);
        let be = rig.kinetics.get(pos(8, 9, 8)).expect("turntable");
        assert_eq!(be.stress, 16384.0);
        assert!(!be.overstressed);
        assert_eq!(be.get_speed(), 256.0);
    }

    /// Two running networks turn a shaft line opposite ways. The shaft placed between them takes
    /// the west side as its source, then breaks on the incompatible east side.
    #[test]
    fn connecting_opposite_networks_breaks_the_new_block() {
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 16, EAST);
        rig.place(&[
            (pos(1, 0, 0), Kind::Shaft(X)),
            (pos(3, 0, 0), Kind::Shaft(X)),
        ]);
        rig.motor(pos(4, 0, 0), 16, WEST);
        rig.ticks(3);
        assert_eq!(rig.speed(pos(1, 0, 0)), 16.0);
        assert_eq!(rig.speed(pos(3, 0, 0)), -16.0);

        rig.place(&[(pos(2, 0, 0), Kind::Shaft(X))]);
        rig.ticks(1);
        assert_eq!(rig.level.destroyed, [pos(2, 0, 0)]);
        assert!(rig.kinetics.get(pos(2, 0, 0)).is_none());
        assert_eq!(rig.speed(pos(1, 0, 0)), 16.0);
        assert_eq!(rig.speed(pos(3, 0, 0)), -16.0);
    }

    /// A ninth doubling would turn a small cog at 512 RPM: the large cog driving it breaks.
    #[test]
    fn overspeed_breaks_the_driving_block() {
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 1, UP);
        for step in 0..9 {
            rig.place(&[
                (pos(step, step + 1, step), Kind::Large(Y)),
                (pos(step + 1, step + 1, step + 1), Kind::Small(Y)),
            ]);
        }
        rig.ticks(3);
        assert_eq!(rig.level.destroyed, [pos(8, 9, 8)]);
        assert_eq!(rig.speed(pos(8, 8, 8)), 256.0);
    }

    #[test]
    fn breaking_the_shaft_stops_everything_past_it() {
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 16, UP);
        rig.place(&[
            (pos(0, 1, 0), Kind::Shaft(Y)),
            (pos(0, 2, 0), Kind::Shaft(Y)),
        ]);
        rig.ticks(3);
        rig.kinetics.remove(&mut rig.level, pos(0, 1, 0));
        rig.ticks(1);
        let be = rig.kinetics.get(pos(0, 2, 0)).expect("shaft");
        assert_eq!((be.speed, be.source, be.network), (0.0, None, None));
        assert_eq!(rig.speed(pos(0, 0, 0)), 16.0);
    }

    /// A three-segment belt running south between X-axis pulleys, driven from the west.
    fn belt_rig() -> Rig {
        const SOUTH: u8 = 3;
        let belt = |part| {
            Kind::Belt(Belt {
                facing: SOUTH,
                slope: Slope::Horizontal,
                part,
            })
        };
        let mut rig = Rig::new();
        rig.motor(pos(0, 0, 0), 16, EAST);
        rig.place(&[
            (pos(1, 0, 0), belt(Part::Start)),
            (pos(1, 0, 1), belt(Part::Middle)),
            (pos(1, 0, 2), belt(Part::End)),
            (pos(2, 0, 2), Kind::Shaft(X)),
        ]);
        rig.ticks(3);
        rig
    }

    /// `initBelt` numbers the chain from its start, and every segment turns with the pulley
    /// that drives it (`BeltBlockEntity.propagateRotationTo`), out to the far pulley's shaft.
    #[test]
    fn belt_segments_share_one_controller_and_speed() {
        let rig = belt_rig();
        for (index, z) in (0..3).enumerate() {
            let be = rig.kinetics.get(pos(1, 0, z)).expect("belt");
            assert_eq!(be.controller, Some(pos(1, 0, 0)));
            assert_eq!((be.belt_length, be.belt_index), (3, index as i32));
            assert_eq!(be.get_speed(), 16.0);
        }
        assert_eq!(rig.speed(pos(2, 0, 2)), 16.0);
    }

    /// `BeltBlock.onRemove`: breaking the middle turns both pulleys back into shafts, and the
    /// shaft past the belt loses its source.
    #[test]
    fn breaking_a_belt_segment_takes_the_chain_with_it() {
        let mut rig = belt_rig();
        rig.kinetics.remove(&mut rig.level, pos(1, 0, 1));
        rig.ticks(1);
        assert_eq!(rig.level.replaced, [pos(1, 0, 2), pos(1, 0, 0)]);
        assert!(rig.kinetics.get(pos(1, 0, 0)).is_none());
        assert!(rig.kinetics.get(pos(1, 0, 2)).is_none());
        assert_eq!(rig.speed(pos(2, 0, 2)), 0.0);
    }
}
