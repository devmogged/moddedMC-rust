//! Train Signal block state and its boundary on a directed railway edge.
use super::{
    math::Vec3,
    railway, runtime,
    sections::{self, Cut},
    station,
    targeting::{self, Selection, TargetUse, coords, pos, state_at},
};
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    block_displays::BlockDisplay,
    content::{self, Block, BlockUpdate, Content, ContentEvent, ShapeBox, State, World},
    interaction::{UseOnBlock, UseResult},
    saved_data::{Key, SavedData},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

pub(crate) const ID: &str = "create:track_signal";
const PROPERTIES: [&str; 4] = [
    "type=entry_signal,powered=false",
    "type=entry_signal,powered=true",
    "type=cross_signal,powered=false",
    "type=cross_signal,powered=true",
];
const LIGHT: [f32; 3] = [0.125, 0.125, 0.125];
static SAVE: OnceLock<(SavedData, Key)> = OnceLock::new();
static STATES: OnceLock<[Option<u32>; 4]> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Signal {
    position: [i32; 3],
    target: Selection,
    axis: [i8; 3],
    powered: bool,
    cross: bool,
}

#[derive(Default, Deserialize, Serialize)]
struct Signals {
    #[serde(rename = "Signals")]
    signals: Vec<Signal>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Aspect {
    Invalid,
    Red,
    Yellow,
    Green,
}

impl Aspect {
    const fn light(self) -> Option<&'static str> {
        match self {
            Self::Invalid => None,
            Self::Red => Some("minecraft:red_concrete"),
            Self::Yellow => Some("minecraft:yellow_concrete"),
            Self::Green => Some("minecraft:lime_concrete"),
        }
    }
}

pub(super) fn register(
    content: Content,
    registry: &mut Registry<'_>,
    saved: SavedData,
) -> Result<(), RegistrationError> {
    let key = saved.register(registry, "create_signals")?;
    SAVE.set((saved, key))
        .map_err(|_| RegistrationError::DuplicateId)?;
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Train Signal",
            item_model: "create:block/track_signal/item",
            entity: ID,
            states: &std::array::from_fn::<_, 4, _>(|i| State {
                properties: PROPERTIES[i],
                physical: "minecraft:stone",
                model: if i < 2 {
                    "create:block/track_signal/block_entry_signal"
                } else {
                    "create:block/track_signal/block_cross_signal"
                },
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[ShapeBox {
                    min: [0, 0, 0],
                    max: [16, 16, 16],
                }],
            }),
        },
        None,
    )
}

fn read(world: World<'_>) -> Option<Signals> {
    let &(save, key) = SAVE.get()?;
    match save.read(&world, key) {
        Some(bytes) => fastnbt::from_bytes(&bytes).ok(),
        None => Some(Signals::default()),
    }
}

fn write(world: World<'_>, signals: &Signals) -> bool {
    let (Some(&(save, key)), Ok(bytes)) = (SAVE.get(), fastnbt::to_bytes(signals)) else {
        return false;
    };
    save.write(&world, key, &bytes)
}

fn states(content: Content) -> &'static [Option<u32>; 4] {
    STATES.get_or_init(|| PROPERTIES.map(|props| content.state(&format!("{ID}[{props}]"))))
}

pub(crate) fn has_state(content: Content, state: u32) -> bool {
    states(content).contains(&Some(state))
}

pub(super) fn claimed(world: World<'_>, selection: &Selection) -> bool {
    read(world).is_none_or(|data| {
        data.signals
            .iter()
            .any(|signal| same_track(&signal.target, selection))
    })
}

fn same_track(a: &Selection, b: &Selection) -> bool {
    a.dimension == b.dimension && a.target == b.target
}

pub(crate) fn use_on_block(context: &mut UseOnBlock<'_>, content: Content) -> UseResult {
    match targeting::use_target(context, content, |world, selection| {
        !station::claimed(world, selection)
    }) {
        TargetUse::Handled(result) => result,
        TargetUse::Place(selection, clicked) => {
            let world = context.world;
            crate::with_world(world, |kinetics| {
                let result = place(context, content, selection, clicked);
                if result == UseResult::Success {
                    kinetics.signal_updates.dirty = true;
                }
                result
            })
        }
    }
}

fn place(
    context: &mut UseOnBlock<'_>,
    content: Content,
    selection: Selection,
    clicked: u32,
) -> UseResult {
    let world = context.world;
    let (Some(mut data), Some(placement)) = (
        read(world),
        targeting::placement(context, content, &selection, clicked),
    ) else {
        return UseResult::Fail;
    };
    if station::claimed(world, &selection) {
        return UseResult::Fail;
    }
    let Some((shape, _)) = state_at(world, pos(selection.target))
        .and_then(|state| runtime::track_state(content, state))
    else {
        return UseResult::Fail;
    };
    let [axis] = shape.axes() else {
        return UseResult::Fail;
    };
    let axis = [axis.x as i8, axis.y as i8, axis.z as i8];
    let shared = data.signals.iter().find(|signal| {
        same_track(&signal.target, &selection) && signal.target.front == selection.front
    });
    let cross = shared.is_some_and(|signal| signal.cross);
    let powered = world.has_neighbor_signal(placement.0);
    let Some(state) = states(content)[usize::from(cross) * 2 + usize::from(powered)] else {
        return UseResult::Fail;
    };
    let position = coords(placement.0);
    data.signals.retain(|signal| {
        signal.position != position || signal.target.dimension != selection.dimension
    });
    data.signals.push(Signal {
        position,
        target: selection.clone(),
        axis,
        powered,
        cross,
    });
    targeting::place_block(context, &selection, placement, state, || {
        write(world, &data)
    })
}

pub(crate) fn wrench(world: World<'_>, content: Content, position: Pos) -> bool {
    crate::with_world(world, |kinetics| {
        let (Some(dimension), Some(mut data)) = (world.dimension_id(), read(world)) else {
            return false;
        };
        let Some(signal) = data.signals.iter().find(|signal| {
            signal.position == coords(position) && signal.target.dimension == dimension
        }) else {
            return false;
        };
        let target = signal.target.clone();
        if !railway::read(&world)
            .is_ok_and(|data| targeting::directed_edge(&data, world, content, &target).is_some())
        {
            return false;
        }
        let cross = !signal.cross;
        for signal in &mut data.signals {
            if same_track(&signal.target, &target) && signal.target.front == target.front {
                signal.cross = cross;
            }
        }
        if !write(world, &data) {
            return false;
        }
        kinetics.signal_updates.dirty = true;
        targeting::sound(world, position, "minecraft:block.lever.click", 1.0);
        true
    })
}

fn aspects(world: World<'_>, content: Content, data: &Signals) -> Option<Vec<Aspect>> {
    let dimension = world.dimension_id()?;
    let graphs = railway::read(&world).ok()?;
    let mut result = vec![Aspect::Invalid; data.signals.len()];
    let mut groups = HashMap::<_, Vec<_>>::new();
    for (index, signal) in data
        .signals
        .iter()
        .enumerate()
        .filter(|(_, s)| s.target.dimension == dimension)
    {
        let [x, y, z] = signal.axis.map(f64::from);
        let located = if state_at(world, pos(signal.target.target)).is_some() {
            targeting::directed_edge(&graphs, world, content, &signal.target)
        } else {
            targeting::locate(&graphs, &signal.target, Vec3 { x, y, z })
        };
        let Some((edge, distance)) = located else {
            continue;
        };
        let Some(graph) = graphs
            .graphs
            .iter()
            .find(|saved| saved.graph.nodes.contains_key(&edge.from))
        else {
            continue;
        };
        groups
            .entry(graph.uuid)
            .or_default()
            .push((index, Cut { edge, distance }));
    }

    for graph in &graphs.graphs {
        let Some(points) = groups.remove(&graph.uuid) else {
            continue;
        };
        let (indices, cuts): (Vec<_>, Vec<_>) = points.into_iter().unzip();
        let sections = sections::sections(&graph.graph, &cuts);
        let forced: HashSet<_> = indices
            .iter()
            .filter_map(|&index| {
                let signal = &data.signals[index];
                signal
                    .powered
                    .then_some((signal.target.target, signal.target.front))
            })
            .collect();
        for (&index, &[before, after]) in indices.iter().zip(&sections) {
            let signal = &data.signals[index];
            result[index] = if before == after {
                Aspect::Invalid
            } else if forced.contains(&(signal.target.target, signal.target.front)) {
                Aspect::Red
            } else {
                Aspect::Green
            };
        }
        // Chain signals inherit downstream aspects. Invalid signals leave a path open.
        let next: Vec<_> = indices
            .iter()
            .enumerate()
            .map(|(i, &index)| {
                if data.signals[index].cross {
                    sections::chained(&graph.graph, &cuts, i)
                } else {
                    Vec::new()
                }
            })
            .collect();
        for _ in 0..indices.len() {
            let previous: Vec<_> = indices.iter().map(|&i| result[i]).collect();
            let mut changed = false;
            for (i, &index) in indices.iter().enumerate() {
                let signal = &data.signals[index];
                if !signal.cross
                    || previous[i] == Aspect::Invalid
                    || forced.contains(&(signal.target.target, signal.target.front))
                {
                    continue;
                }
                let mut red = false;
                let mut open = false;
                for &j in &next[i] {
                    red |= matches!(previous[j], Aspect::Red | Aspect::Yellow);
                    open |= previous[j] != Aspect::Red;
                }
                let aspect = if !red {
                    Aspect::Green
                } else if open {
                    Aspect::Yellow
                } else {
                    Aspect::Red
                };
                changed |= result[index] != aspect;
                result[index] = aspect;
            }
            if !changed {
                break;
            }
        }
    }
    Some(result)
}

#[derive(Default)]
pub(crate) struct SignalUpdates {
    loaded: HashSet<Pos>,
    lights: HashMap<Pos, (Aspect, i32)>,
    low_at: HashMap<Pos, u64>,
    dirty: bool,
}

impl SignalUpdates {
    pub(crate) fn tick(
        &mut self,
        world: World<'_>,
        content: Content,
        time: u64,
        events: &[ContentEvent],
        rails_changed: bool,
    ) {
        let mut removed = Vec::new();
        self.dirty |= rails_changed && !self.loaded.is_empty();
        for event in events {
            let position = event.position;
            match event.reason {
                content::UNLOADED if self.loaded.remove(&position) => {
                    self.clear_light(world, position);
                    self.low_at.remove(&position);
                }
                content::LOADED | content::STATE_CHANGED | content::MOVED_STATE_CHANGED
                    if has_state(content, event.state) =>
                {
                    self.dirty |= self.loaded.insert(position);
                }
                content::STATE_CHANGED | content::MOVED_STATE_CHANGED
                    if has_state(content, event.previous) =>
                {
                    self.loaded.remove(&position);
                    self.low_at.remove(&position);
                    self.clear_light(world, position);
                    if state_at(world, position).is_some_and(|state| !has_state(content, state)) {
                        removed.push(coords(position));
                    }
                    self.dirty = true;
                }
                content::NEIGHBOR_CHANGED if self.loaded.contains(&position) => self.dirty = true,
                _ => {}
            }
        }
        if !self.dirty && !self.low_at.values().any(|&at| at <= time) {
            return;
        }
        let (Some(dimension), Some(mut data)) = (world.dimension_id(), read(world)) else {
            return;
        };
        let count = data.signals.len();
        data.signals.retain(|signal| {
            signal.target.dimension != dimension || !removed.contains(&signal.position)
        });
        let mut persist = count != data.signals.len();
        for signal in data
            .signals
            .iter_mut()
            .filter(|signal| signal.target.dimension == dimension)
        {
            let position = pos(signal.position);
            if !self.loaded.contains(&position) {
                continue;
            }
            let powered = world.has_neighbor_signal(position);
            if powered || !signal.powered {
                self.low_at.remove(&position);
            } else if time
                < *self
                    .low_at
                    .entry(position)
                    .or_insert(time.saturating_add(4))
            {
                continue;
            } else {
                self.low_at.remove(&position);
            }
            if signal.powered != powered {
                signal.powered = powered;
                persist = true;
            }
            let index = usize::from(signal.cross) * 2 + usize::from(powered);
            if let Some(state) = states(content)[index]
                && state_at(world, position) != Some(state)
            {
                world.write(&[BlockUpdate {
                    position,
                    state,
                    flags: content::UPDATE_CLIENTS,
                }]);
            }
        }
        self.dirty = false;
        if persist {
            self.dirty = !write(world, &data);
        }
        let Some(aspects) = aspects(world, content, &data) else {
            return;
        };
        for (signal, aspect) in data.signals.iter().zip(aspects) {
            let position = pos(signal.position);
            if signal.target.dimension == dimension && self.loaded.contains(&position) {
                self.show_light(world, content, position, aspect);
            }
        }
    }

    fn clear_light(&mut self, world: World<'_>, position: Pos) {
        if let Some((_, id)) = self.lights.remove(&position)
            && let Some(displays) = crate::BLOCK_DISPLAYS.get()
        {
            displays.remove(world, id);
        }
    }

    fn show_light(&mut self, world: World<'_>, content: Content, position: Pos, aspect: Aspect) {
        if self
            .lights
            .get(&position)
            .is_some_and(|(current, _)| *current == aspect)
        {
            return;
        }
        self.clear_light(world, position);
        let (Some(displays), Some(state)) = (
            crate::BLOCK_DISPLAYS.get(),
            aspect.light().and_then(|light| content.state(light)),
        ) else {
            return;
        };
        if let Some(id) = displays.spawn(
            world,
            state,
            &BlockDisplay {
                position: coords(position).map(f64::from),
                translation: [0.4375, 1.0, 0.4375],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: LIGHT,
                transform_interpolation: 0,
                position_interpolation: 0,
                brightness: 15 << 4 | 15 << 20,
            },
        ) {
            self.lights.insert(position, (aspect, id));
        }
    }
}

pub(crate) fn describe(world: World<'_>, content: Content, position: Pos) -> Option<String> {
    let dimension = world.dimension_id()?;
    if !has_state(content, state_at(world, position)?) {
        return None;
    }
    let data = read(world)?;
    let index = data
        .signals
        .iter()
        .position(|s| s.position == coords(position) && s.target.dimension == dimension)?;
    let aspect = aspects(world, content, &data)?[index];
    let signal = &data.signals[index];
    let kind = if signal.cross { "Cross" } else { "Entry" };
    let [x, y, z] = signal.target.target;
    Some(format!(
        "Train Signal targets {x} {y} {z} {kind} {aspect:?} Powered={}",
        signal.powered
    ))
}
