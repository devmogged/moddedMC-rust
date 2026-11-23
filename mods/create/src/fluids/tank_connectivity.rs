//! Create 6.0.10 ConnectivityHandler for vertical fluid tanks.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{LazyLock, Mutex, OnceLock, PoisonError},
};

use fastnbt::Value;
use moddedmc_api::{
    InterfaceUnavailable, Pos, RegistrationError, Registry,
    content::{self, ContentEvent, World},
    saved_data::{self, Key, SavedData},
};
use serde::{Deserialize, Serialize};

use super::{
    network::FluidHandler,
    pipe_connection::FluidStack,
    runtime::is_tank,
    tank::{MAX_WIDTH, TankInventory},
    tank_runtime::{
        CAPACITY_PER_BLOCK, MAX_HEIGHT, controller, controller_from_tag, from_tag, load, new_tank,
        parts, read_tag, update_groups,
    },
};
use crate::{FACES, offset};

type Splits = HashMap<Pos, Option<TankInventory>>;
static REMOVED: LazyLock<Mutex<HashMap<usize, Splits>>> = LazyLock::new(Mutex::default);
static SAVED: OnceLock<SavedData> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = SAVED.set(saved_data::require()?);
    Ok(())
}

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let saved = SAVED.get().ok_or(RegistrationError::InvalidDefinition)?;
    let key = saved.register(registry, "tank_splits")?;
    KEY.set(key).map_err(|_| RegistrationError::DuplicateId)
}

/// Read the disappearing block entity before the engine clears it.
pub fn replaced(world: World<'_>, event: &ContentEvent) {
    if event.reason == content::MOVED_STATE_CHANGED || !is_tank(event.previous) {
        return;
    }
    let Some(data) = read_tag(world, event.position) else {
        return;
    };
    let Some(owner) = controller_from_tag(&data, event.position) else {
        return;
    };
    let removed = if owner == event.position {
        let Some(tank) = from_tag(&data) else { return };
        if tank.width() == 1 && tank.height() == 1 {
            return;
        }
        Some(tank)
    } else {
        None
    };
    let mut worlds = REMOVED.lock().unwrap_or_else(PoisonError::into_inner);
    insert_split(worlds.entry(world.id()).or_default(), owner, removed);
}

fn insert_split(splits: &mut Splits, owner: Pos, removed: Option<TankInventory>) {
    let before = splits.entry(owner).or_default();
    if removed.is_some() {
        *before = removed;
    }
}

#[derive(Deserialize, Serialize)]
struct SplitRecord {
    origin: [i32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tank: Option<HashMap<String, Value>>,
}

#[derive(Deserialize, Serialize)]
struct SplitFile {
    #[serde(rename = "Splits")]
    records: Vec<SplitRecord>,
}

#[derive(Default)]
pub struct Connectivity {
    splits: Splits,
    initialized: bool,
    dirty: bool,
}

impl Connectivity {
    pub fn pending(&self, owner: Pos) -> bool {
        self.splits.contains_key(&owner)
    }

    pub fn tick(&mut self, world: World<'_>, time: u64, events: &[ContentEvent]) {
        let Some((&saved, &key)) = SAVED.get().zip(KEY.get()) else {
            return;
        };
        if !self.initialized {
            self.initialized = true;
            if let Some(bytes) = saved.read(&world, key) {
                match fastnbt::from_bytes::<SplitFile>(&bytes) {
                    Ok(data) => {
                        for record in data.records {
                            let [x, y, z] = record.origin;
                            let tank = record.tank.as_ref().and_then(from_tag);
                            self.splits.insert(Pos { x, y, z }, tank);
                        }
                    }
                    Err(_) => moddedmc_api::info("Create: invalid tank split data"),
                }
            }
        }
        if let Some(removed) = REMOVED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&world.id())
        {
            for (owner, snapshot) in removed {
                insert_split(&mut self.splits, owner, snapshot);
            }
            self.dirty = true;
        }
        let mut seeds: Vec<_> = events
            .iter()
            .filter(|event| {
                matches!(event.reason, content::STATE_CHANGED | content::LOADED)
                    && is_tank(event.state)
                    && !is_tank(event.previous)
            })
            .map(|event| event.position)
            .collect();
        if self.dirty || !seeds.is_empty() || time.is_multiple_of(20) {
            let mut owners: Vec<_> = self.splits.keys().copied().collect();
            owners.sort_by_key(|p| (p.y, p.x, p.z));
            for owner in owners {
                if let Some(parts) = split(world, owner, self.splits[&owner].as_ref()) {
                    self.splits.remove(&owner);
                    self.dirty = true;
                    seeds.extend(parts);
                }
            }
        }
        if self.dirty {
            let records = self
                .splits
                .iter()
                .map(|(p, tank)| SplitRecord {
                    origin: [p.x, p.y, p.z],
                    tank: tank.as_ref().map(TankInventory::write_controller_nbt),
                })
                .collect();
            self.dirty = !fastnbt::to_bytes(&SplitFile { records })
                .is_ok_and(|bytes| saved.write(&world, key, &bytes));
        }
        form(world, &self.splits, seeds);
    }
}

/// Check the entire volume before reallocating any fluid across controller boundaries.
fn live_members(world: World<'_>, owner: Pos, tank: &TankInventory) -> Option<Vec<Pos>> {
    let positions: Vec<_> = parts(owner, tank.width(), tank.height()).collect();
    let mut states = vec![content::UNLOADED_STATE; positions.len()];
    if !world.read(&positions, &mut states) || states.contains(&content::UNLOADED_STATE) {
        return None;
    }
    Some(
        positions
            .into_iter()
            .zip(states)
            .filter(|&(pos, state)| is_tank(state) && controller(world, pos) == Some(owner))
            .map(|(pos, _)| pos)
            .collect(),
    )
}

fn singles(
    source: &mut TankInventory,
    positions: impl IntoIterator<Item = Pos>,
) -> Option<Vec<(Pos, TankInventory)>> {
    positions
        .into_iter()
        .map(|pos| {
            let mut tank = new_tank()?;
            tank.set_window(source.window());
            if let Some(fluid) = source.drain_any(CAPACITY_PER_BLOCK, false) {
                tank.fill(&fluid, false);
            }
            Some((pos, tank))
        })
        .collect()
}

fn split(world: World<'_>, owner: Pos, removed: Option<&TankInventory>) -> Option<Vec<Pos>> {
    let mut tank = if let Some(tank) = removed {
        tank.clone()
    } else {
        let mut state = [content::UNLOADED_STATE];
        if !world.read(&[owner], &mut state) || state[0] == content::UNLOADED_STATE {
            return None;
        }
        if !is_tank(state[0]) {
            return Some(Vec::new());
        }
        load(world, owner)?
    };
    let mut survivors = live_members(world, owner, &tank)?;
    if removed.is_some() {
        survivors.retain(|&pos| pos != owner);
    }
    let groups = singles(&mut tank, survivors.iter().copied())?;
    (groups.is_empty() || update_groups(world, &groups)).then_some(survivors)
}

#[derive(Clone)]
struct Shape {
    width: u8,
    height: u16,
    owners: Vec<Pos>,
}

impl Shape {
    fn volume(&self) -> usize {
        usize::from(self.width).pow(2) * usize::from(self.height)
    }
}

struct Search<'a> {
    splits: &'a Splits,
    world: World<'a>,
    parts: HashMap<Pos, Option<Pos>>,
    inventories: HashMap<Pos, Option<TankInventory>>,
}

impl<'a> Search<'a> {
    fn new(world: World<'a>, splits: &'a Splits) -> Self {
        Self {
            splits,
            world,
            parts: HashMap::new(),
            inventories: HashMap::new(),
        }
    }

    fn owner(&mut self, pos: Pos) -> Option<Pos> {
        *self
            .parts
            .entry(pos)
            .or_insert_with(|| controller(self.world, pos))
    }

    fn tank(&mut self, owner: Pos) -> Option<&TankInventory> {
        if self.splits.contains_key(&owner) {
            return None;
        }
        self.inventories
            .entry(owner)
            .or_insert_with(|| load(self.world, owner))
            .as_ref()
    }

    fn shape(&mut self, origin: Pos, width: u8) -> Shape {
        let size = i32::from(width);
        let mut shape = Shape {
            width,
            height: 0,
            owners: Vec::new(),
        };
        let mut seen = HashSet::new();
        let mut fluid: Option<FluidStack> = None;
        for y in 0..MAX_HEIGHT {
            let mut layer = Vec::new();
            for pos in parts(offset(origin, [0, i32::from(y), 0]), width, 1) {
                let Some(owner) = self.owner(pos) else {
                    return shape;
                };
                let Some(tank) = self.tank(owner) else {
                    return shape;
                };
                let other_width = i32::from(tank.width());
                if owner.x < origin.x
                    || owner.z < origin.z
                    || owner.x + other_width > origin.x + size
                    || owner.z + other_width > origin.z + size
                    || (owner != origin && tank.width() == width && tank.height() == MAX_HEIGHT)
                {
                    return shape;
                }
                if seen.insert(owner) {
                    if let Some(other) = tank.fluid() {
                        if fluid
                            .as_ref()
                            .is_some_and(|f| !f.same_fluid_same_components(other))
                        {
                            return shape;
                        }
                        fluid = Some(other.clone());
                    }
                    layer.push(owner);
                }
            }
            shape.owners.extend(layer);
            shape.height += 1;
        }
        shape
    }

    fn best(&mut self, origin: Pos) -> Option<Shape> {
        if self.owner(origin)? != origin {
            return None;
        }
        let tank = self.tank(origin)?;
        let width = tank.width();
        let current = usize::from(width).pow(2) * usize::from(tank.height());
        (1..=MAX_WIDTH)
            .map(|width| self.shape(origin, width))
            .max_by_key(Shape::volume)
            .filter(|shape| {
                shape.volume() > current || (shape.volume() == current && shape.width > width)
            })
    }
}

fn form(world: World<'_>, splits: &Splits, seeds: Vec<Pos>) {
    if seeds.is_empty() {
        return;
    }
    let mut search = Search::new(world, splits);
    let mut candidates = Vec::new();
    let mut scored = HashSet::new();
    for seed in seeds {
        let mut frontier = VecDeque::from([seed]);
        let mut visited = HashSet::new();
        while let Some(pos) = frontier.pop_front() {
            if !visited.insert(pos) || search.owner(pos).is_none() {
                continue;
            }
            if scored.insert(pos)
                && let Some(shape) = search.best(pos)
            {
                candidates.push((pos, shape.volume()));
            }
            for face in [4, 0, 2] {
                let next = offset(pos, FACES[face]);
                if seed.x - next.x < i32::from(MAX_WIDTH) && seed.z - next.z < i32::from(MAX_WIDTH)
                {
                    frontier.push_back(next);
                }
            }
        }
    }
    candidates.sort_by_key(|&(_, volume)| std::cmp::Reverse(volume));
    for (origin, _) in candidates {
        if let Some(shape) = search.best(origin)
            && let Some(groups) = merge_groups(world, origin, &shape)
            && update_groups(world, &groups)
        {
            search.parts.clear();
            search.inventories.clear();
        }
    }
}

fn merge_groups(world: World<'_>, origin: Pos, shape: &Shape) -> Option<Vec<(Pos, TankInventory)>> {
    let selected: HashSet<_> = parts(origin, shape.width, shape.height).collect();
    let mut joined = new_tank()?;
    joined.resize(shape.width, shape.height).ok()?;
    joined.set_window(false);
    let mut groups = Vec::new();
    let mut absorbed = 0;
    for &owner in &shape.owners {
        let mut tank = load(world, owner)?;
        let present = live_members(world, owner, &tank)?;
        if present.len() != usize::from(tank.width()).pow(2) * usize::from(tank.height()) {
            return None;
        }
        let remaining: Vec<_> = present
            .into_iter()
            .filter(|pos| !selected.contains(pos))
            .collect();
        let capacity_left = remaining.len() as u32 * CAPACITY_PER_BLOCK;
        absorbed += (tank.capacity() - capacity_left) / CAPACITY_PER_BLOCK;
        let transfer = if selected.contains(&owner) {
            tank.capacity() - capacity_left
        } else {
            tank.amount().saturating_sub(capacity_left)
        };
        if let Some(fluid) = tank.drain_any(transfer, false)
            && joined.fill(&fluid, false) != fluid.amount
        {
            return None;
        }
        joined.set_window(joined.window() || tank.window());
        groups.extend(singles(&mut tank, remaining)?);
    }
    if absorbed as usize != selected.len() {
        return None;
    }
    groups.push((origin, joined));
    Some(groups)
}
