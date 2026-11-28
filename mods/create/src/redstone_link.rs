//! Create 6.0.10's Redstone Link and `RedstoneLinkNetworkHandler`.

use std::{
    collections::{HashMap, hash_map::Entry},
    sync::{Mutex, PoisonError},
};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{
        self, Block, BlockMaterial, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State,
        World,
    },
    effects::SOUND_BLOCKS,
    interaction::{BlockUseResult, RedstoneQuery, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    DIRECTIONS, EFFECTS, StateIds, WRENCH,
    belt_inventory::{self, TransportedItemStack, with_count},
    belt_tunnel::{read, shift},
    directed_detector::orientation,
    filtering,
    processing_recipes::item_id,
};

pub(crate) const ID: &str = "create:redstone_link";
const LINKED_CONTROLLER: &str = "create:linked_controller";
const UP: u8 = 1;
/// `CLogistics.linkRange`.
const LINK_RANGE: i64 = 256;
/// `RedstoneLinkFrequencySlot.getScale()` over `ValueBoxTransform.Dual.testHit`'s 3.5.
const SLOT_RADIUS: f32 = 0.4975 / 3.5;
/// `RedstoneLinkFrequencySlot` places both slots 3.01 px in front of the attached side.
const SLOT_DEPTH: f32 = 3.01 / 16.0;
const SLOT_HIGH: f32 = 10.5 / 16.0;
const SLOT_LOW: f32 = 5.5 / 16.0;
const FREQUENCY_SCALE: f32 = 0.24;
/// Blockstate rotations from the release `redstone_link.json`, by facing.
const ROTATIONS: [(i16, i16); 6] = [
    (180, 180),
    (0, 180),
    (270, 180),
    (270, 0),
    (270, 90),
    (270, 270),
];
/// `AllShapes.REDSTONE_BRIDGE`, by facing.
const SHAPES: [ShapeBox; 6] = [
    ShapeBox {
        min: [2, 13, 2],
        max: [14, 17, 14],
    },
    ShapeBox {
        min: [2, -1, 2],
        max: [14, 3, 14],
    },
    ShapeBox {
        min: [3, 1, 13],
        max: [13, 15, 17],
    },
    ShapeBox {
        min: [3, 1, -1],
        max: [13, 15, 3],
    },
    ShapeBox {
        min: [13, 1, 3],
        max: [17, 15, 13],
    },
    ShapeBox {
        min: [-1, 1, 3],
        max: [3, 15, 13],
    },
];

/// Received strength by world and receiver position, read by redstone queries while the
/// world's kinetics are locked.
static RECEIVED: Mutex<Option<HashMap<(usize, Pos), u8>>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct LinkState {
    pub(crate) facing: u8,
    pub(crate) powered: bool,
    pub(crate) receiver: bool,
}

impl LinkState {
    fn all() -> impl Iterator<Item = Self> {
        (0..6).flat_map(|facing| {
            [false, true].into_iter().flat_map(move |powered| {
                [false, true].map(|receiver| Self {
                    facing,
                    powered,
                    receiver,
                })
            })
        })
    }

    fn properties(self) -> String {
        format!(
            "facing={},powered={},receiver={}",
            DIRECTIONS[usize::from(self.facing)],
            self.powered,
            self.receiver
        )
    }

    fn id(self, states: &StateIds) -> Option<u32> {
        states.lookup(&format!("{ID}[{}]", self.properties()))
    }

    fn model(self) -> String {
        format!(
            "create:block/redstone_link/{}{}{}",
            if self.receiver {
                "receiver"
            } else {
                "transmitter"
            },
            if self.facing >= 2 { "_vertical" } else { "" },
            if self.powered { "_powered" } else { "" }
        )
    }

    const fn attached(self) -> u8 {
        self.facing ^ 1
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let rows: Vec<_> = LinkState::all()
        .map(|state| (state, state.properties(), state.model()))
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(state, properties, model)| {
            let (x, y) = ROTATIONS[usize::from(state.facing)];
            // Drawn as a display so all 24 states share the display-only carrier.
            State {
                properties,
                physical: "minecraft:stripped_spruce_wood",
                model: "",
                rotating_model: model,
                x: 0,
                y: 0,
                orientation: orientation(x, y),
                shape: std::slice::from_ref(&SHAPES[usize::from(state.facing)]),
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Redstone Link",
            entity: ID,
            item_model: "create:item/redstone_link",
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    content.set_block_material(
        registry,
        ID,
        &BlockMaterial {
            template: "minecraft:stripped_spruce_wood",
            hardness: None,
            map_color: Some("terracotta_brown"),
        },
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for state in LinkState::all() {
        states.add(content, ID, &state.properties(), None);
        if let Some(id) = state.id(states) {
            states.links.insert(id, state);
        }
    }
}

/// `RedstoneLinkBlock.getStateForPlacement`: facing the clicked face. `Placement::face` points
/// toward the clicked block.
pub(crate) fn place(args: &Placement<'_>, states: &StateIds) -> Option<u32> {
    LinkState {
        facing: args.face ^ 1,
        powered: false,
        receiver: false,
    }
    .id(states)
}

/// `isSignalSource() && canConnectRedstone(side)`.
pub(crate) const fn connects(state: LinkState) -> bool {
    state.powered && state.receiver
}

/// `RedstoneLinkBlock.getSignal` and `getDirectSignal`.
pub(crate) fn signal(state: LinkState, query: &RedstoneQuery<'_>) -> u8 {
    if !state.receiver || (query.direct && query.direction != state.facing) {
        return 0;
    }
    lock_received()
        .as_ref()
        .and_then(|received| received.get(&(query.world.id(), query.position)))
        .copied()
        .unwrap_or(0)
}

fn lock_received() -> std::sync::MutexGuard<'static, Option<HashMap<(usize, Pos), u8>>> {
    RECEIVED.lock().unwrap_or_else(PoisonError::into_inner)
}

fn set_received(world: World<'_>, pos: Pos, strength: Option<u8>) {
    let mut received = lock_received();
    let map = received.get_or_insert_with(HashMap::new);
    match strength {
        Some(strength) if strength > 0 => {
            map.insert((world.id(), pos), strength);
        }
        _ => {
            map.remove(&(world.id(), pos));
        }
    }
}

fn empty_stack() -> Value {
    Value::Compound(HashMap::new())
}

#[derive(Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Data {
    transmitter: bool,
    receive: i32,
    received_changed: bool,
    transmit: i32,
    frequency_first: Value,
    frequency_last: Value,
    last_known_position: i64,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            transmitter: true,
            receive: 0,
            received_changed: false,
            transmit: 0,
            frequency_first: empty_stack(),
            frequency_last: empty_stack(),
            last_known_position: 0,
        }
    }
}

/// `RedstoneLinkNetworkHandler.Frequency`: the item and its dye color.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Frequency(Option<String>, i32);

impl Frequency {
    fn of(stack: &Value) -> Self {
        let color = filtering::compound(stack)
            .and_then(|fields| filtering::compound(fields.get("components")?))
            .and_then(|parts| parts.get("minecraft:dyed_color"))
            .and_then(|color| match color {
                Value::Int(rgb) => Some(*rgb),
                Value::Compound(fields) => match fields.get("rgb") {
                    Some(Value::Int(rgb)) => Some(*rgb),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or(-1);
        Self(item_id(stack).map(str::to_owned), color)
    }
}

pub(crate) struct Link {
    data: Data,
    key: (Frequency, Frequency),
    displays: [Option<TransportedItemStack>; 2],
    dirty: bool,
}

impl Link {
    fn load(world: World<'_>, pos: Pos, state: LinkState) -> Self {
        let mut data = world
            .entity(pos)
            .as_deref()
            .and_then(|bytes| fastnbt::from_bytes::<Data>(bytes).ok())
            .unwrap_or_default();
        data.receive = data.receive.clamp(0, 15);
        data.transmit = data.transmit.clamp(0, 15);
        data.transmitter = !state.receiver;
        let key = (
            Frequency::of(&data.frequency_first),
            Frequency::of(&data.frequency_last),
        );
        Self {
            data,
            key,
            displays: [None, None],
            dirty: true,
        }
    }

    const fn transmitted(&self) -> i32 {
        if self.data.transmitter {
            self.data.transmit
        } else {
            0
        }
    }

    fn set_frequency(&mut self, first: bool, stack: Option<&Value>) -> bool {
        let stack = stack.map_or_else(empty_stack, |stack| with_count(stack, 1));
        let slot = if first {
            &mut self.data.frequency_first
        } else {
            &mut self.data.frequency_last
        };
        if *slot == stack {
            return false;
        }
        *slot = stack;
        self.key = (
            Frequency::of(&self.data.frequency_first),
            Frequency::of(&self.data.frequency_last),
        );
        self.dirty = true;
        true
    }

    fn remove_displays(&mut self, world: World<'_>) {
        for display in self.displays.iter_mut().filter_map(Option::take) {
            belt_inventory::remove_display(world, &display);
        }
    }
}

/// `Vec3i.closerThan(other, linkRange)`.
fn within_range(a: Pos, b: Pos) -> bool {
    let [dx, dy, dz] = [a.x - b.x, a.y - b.y, a.z - b.z].map(i64::from);
    dx * dx + dy * dy + dz * dz < LINK_RANGE * LINK_RANGE
}

/// `RedstoneLinkNetworkHandler.updateNetworkOf`, with `actor` already added or removed.
fn update_network(
    world: World<'_>,
    links: &mut HashMap<Pos, Link>,
    actor: Pos,
    key: &(Frequency, Frequency),
) {
    let power = links
        .iter()
        .filter(|(pos, link)| link.key == *key && within_range(actor, **pos))
        .map(|(_, link)| link.transmitted())
        .max()
        .unwrap_or(0)
        .min(15);
    for (&pos, link) in links.iter_mut() {
        if link.key != *key || link.data.transmitter || !within_range(actor, pos) {
            continue;
        }
        if link.data.receive != power {
            link.data.receive = power;
            link.data.received_changed = true;
            link.dirty = true;
            set_received(world, pos, u8::try_from(power).ok());
        }
    }
}

/// `RedstoneLinkBlock.getPower`, including the dust-shaped query on every unattached side.
fn input_power(world: World<'_>, pos: Pos, state: LinkState) -> i32 {
    let mut power = 0;
    for direction in 0..6 {
        power = power.max(world.signal(shift(pos, direction), direction));
    }
    for direction in (0..6).filter(|&direction| direction != state.attached()) {
        power = power.max(world.signal(shift(pos, direction), UP));
    }
    i32::from(power)
}

fn write(world: World<'_>, states: &StateIds, pos: Pos, state: LinkState, flags: u8) {
    if let Some(id) = state.id(states) {
        world.write(&[BlockUpdate {
            position: pos,
            state: id,
            flags,
        }]);
    }
}

/// `RedstoneLinkBlock.updateTransmittedSignal`.
fn update_transmitted(
    world: World<'_>,
    states: &StateIds,
    links: &mut HashMap<Pos, Link>,
    pos: Pos,
    state: LinkState,
) {
    if state.receiver {
        return;
    }
    let power = input_power(world, pos, state);
    if state.powered != (power > 0) {
        write(
            world,
            states,
            pos,
            LinkState {
                powered: power > 0,
                ..state
            },
            content::UPDATE_CLIENTS,
        );
    }
    transmit(world, links, pos, power);
}

/// `RedstoneLinkBlockEntity.transmit`.
fn transmit(world: World<'_>, links: &mut HashMap<Pos, Link>, pos: Pos, power: i32) {
    let Some(link) = links.get_mut(&pos) else {
        return;
    };
    if link.data.transmit != power {
        link.data.transmit = power;
        link.dirty = true;
    }
    let key = link.key.clone();
    update_network(world, links, pos, &key);
}

/// `RedstoneLinkBlockEntity.updateSelfAndAttached`.
fn update_self_and_attached(world: World<'_>, pos: Pos, state: LinkState) {
    world.update_neighbors(pos);
    world.update_neighbors(shift(pos, state.attached()));
}

pub(crate) fn event(
    world: World<'_>,
    states: &StateIds,
    links: &mut HashMap<Pos, Link>,
    event: &ContentEvent,
) {
    let pos = event.position;
    // Events arrive a tick late; act on the live state so a stale one is never written back.
    let live = (event.reason != content::UNLOADED)
        .then(|| states.links.get(&read(world, pos)).copied())
        .flatten();
    match live {
        Some(state) => {
            if let Entry::Vacant(entry) = links.entry(pos) {
                let link = entry.insert(Link::load(world, pos, state));
                set_received(
                    world,
                    pos,
                    (!link.data.transmitter)
                        .then(|| u8::try_from(link.data.receive).ok())
                        .flatten(),
                );
                let key = link.key.clone();
                // `LinkBehaviour.initialize` joins the network.
                update_network(world, links, pos, &key);
                if event.reason == content::STATE_CHANGED
                    && !states.links.contains_key(&event.previous)
                {
                    // `onPlace`.
                    update_transmitted(world, states, links, pos, state);
                }
            }
            if event.reason == content::NEIGHBOR_CHANGED {
                neighbor_changed(world, states, links, pos, state);
            }
        }
        _ => {
            let Some(mut link) = links.remove(&pos) else {
                return;
            };
            link.remove_displays(world);
            set_received(world, pos, None);
            update_network(world, links, pos, &link.key);
            if event.reason != content::UNLOADED
                && let Some(&previous) = states
                    .links
                    .get(&event.previous)
                    .or_else(|| states.links.get(&event.state))
            {
                update_self_and_attached(world, pos, previous);
            }
        }
    }
}

/// `RedstoneLinkBlock.neighborChanged` followed by its scheduled `tick`.
fn neighbor_changed(
    world: World<'_>,
    states: &StateIds,
    links: &mut HashMap<Pos, Link>,
    pos: Pos,
    state: LinkState,
) {
    let support = read(world, shift(pos, state.attached()));
    if support != content::UNLOADED_STATE && states.replaceable.contains(&support) {
        world.destroy(pos, true);
        return;
    }
    update_transmitted(world, states, links, pos, state);
    if !state.receiver {
        update_self_and_attached(world, pos, state);
    }
}

/// `RedstoneLinkBlockEntity.tick` and the frequency displays.
pub(crate) fn tick(world: World<'_>, states: &StateIds, links: &mut HashMap<Pos, Link>) {
    let positions: Vec<Pos> = links.keys().copied().collect();
    for pos in positions {
        let Some(&state) = states.links.get(&read(world, pos)) else {
            continue;
        };
        let Some(link) = links.get_mut(&pos) else {
            continue;
        };
        if link.data.transmitter == state.receiver {
            link.data.transmitter = !state.receiver;
            link.dirty = true;
            set_received(
                world,
                pos,
                state
                    .receiver
                    .then(|| u8::try_from(link.data.receive).ok())
                    .flatten(),
            );
            let key = link.key.clone();
            update_network(world, links, pos, &key);
        }
        let Some(link) = links.get_mut(&pos) else {
            continue;
        };
        if state.receiver {
            let mut current = state;
            if (link.data.receive > 0) != state.powered {
                link.data.received_changed = true;
                current.powered = link.data.receive > 0;
                write(world, states, pos, current, content::UPDATE_ALL);
            }
            if link.data.received_changed {
                link.data.received_changed = false;
                link.dirty = true;
                update_self_and_attached(world, pos, current);
            }
        }
        link.data.last_known_position = pos_as_long(pos);
        if link.dirty
            && let Ok(bytes) = fastnbt::to_bytes(&link.data)
            && world.set_entity(pos, &bytes)
        {
            link.dirty = false;
        }
        render(world, pos, state, link);
    }
}

/// `BlockPos.asLong`.
const fn pos_as_long(pos: Pos) -> i64 {
    ((pos.x as i64 & 0x3FF_FFFF) << 38)
        | ((pos.z as i64 & 0x3FF_FFFF) << 12)
        | (pos.y as i64 & 0xFFF)
}

/// `RedstoneLinkFrequencySlot.getLocalOffset`.
fn slot_offset(facing: u8, first: bool) -> [f32; 3] {
    let height = if first { SLOT_HIGH } else { SLOT_LOW };
    match facing {
        0 => [0.5, 1.0 - SLOT_DEPTH, 1.0 - height],
        1 => [0.5, SLOT_DEPTH, height],
        2 => [0.5, height, 1.0 - SLOT_DEPTH],
        3 => [0.5, height, SLOT_DEPTH],
        4 => [1.0 - SLOT_DEPTH, height, 0.5],
        _ => [SLOT_DEPTH, height, 0.5],
    }
}

/// `ValueBoxTransform.Dual.testHit`, ignoring depth: stock clients aim at the carrier's full
/// outline, not the plate.
fn slot_hit(facing: u8, first: bool, hit: [f32; 3]) -> bool {
    let slot = slot_offset(facing, first);
    let depth = usize::from(crate::kinetics::face_axis(facing));
    let distance: f32 = (0..3)
        .filter(|&axis| axis != depth)
        .map(|axis| (hit[axis] - slot[axis]).powi(2))
        .sum();
    distance.sqrt() < SLOT_RADIUS
}

fn render(world: World<'_>, pos: Pos, state: LinkState, link: &mut Link) {
    for (index, first) in [true, false].into_iter().enumerate() {
        let stack = if first {
            &link.data.frequency_first
        } else {
            &link.data.frequency_last
        };
        let sample = item_id(stack).is_some().then_some(stack);
        let [x, y, z] = slot_offset(state.facing, first).map(f64::from);
        let position = [
            f64::from(pos.x) + x,
            f64::from(pos.y) + y,
            f64::from(pos.z) + z,
        ];
        let mut shown = if state.facing >= 2 {
            belt_inventory::item_display(position, filtering::preview_angle(state.facing), true)
        } else {
            belt_inventory::item_display(position, 0, false)
        };
        if state.facing == 0 {
            shown.rotation = orientation(180, 0);
        }
        shown.scale = FREQUENCY_SCALE;
        filtering::render(world, sample, &mut link.displays[index], shown);
    }
}

/// `RedstoneLinkBlock.toggleMode`, then the scheduled tick's transmitter update.
fn toggle_mode(
    world: World<'_>,
    states: &StateIds,
    links: &mut HashMap<Pos, Link>,
    pos: Pos,
    state: LinkState,
) {
    let next = LinkState {
        receiver: !state.receiver,
        powered: world.has_neighbor_signal(pos),
        ..state
    };
    let power = if state.receiver {
        0
    } else {
        input_power(world, pos, state)
    };
    // Neighbors read the received strength during the state write, so set it first.
    if let Some(link) = links.get_mut(&pos) {
        link.data.transmitter = !next.receiver;
        link.dirty = true;
        set_received(
            world,
            pos,
            next.receiver
                .then(|| u8::try_from(link.data.receive).ok())
                .flatten(),
        );
    }
    transmit(world, links, pos, power);
    write(world, states, pos, next, content::UPDATE_ALL);
    update_transmitted(world, states, links, pos, next);
    if !next.receiver {
        update_self_and_attached(world, pos, next);
    }
}

/// `LinkHandler.onBlockActivated`, `useWithoutItem` and `onWrenched`.
pub(crate) fn use_block(
    context: &mut UseBlock<'_>,
    states: &StateIds,
    state: LinkState,
    links: &mut HashMap<Pos, Link>,
) -> BlockUseResult {
    let pos = context.position;
    links
        .entry(pos)
        .or_insert_with(|| Link::load(context.world, pos, state));
    let item = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok());
    let held = item.as_ref().and_then(item_id);
    if !context.with_item {
        if context.sneaking {
            toggle_mode(context.world, states, links, pos, state);
            return BlockUseResult::Success;
        }
        return BlockUseResult::Pass;
    }
    if context.sneaking || !context.main_hand {
        return BlockUseResult::TryEmptyHand;
    }
    if held == Some(WRENCH) {
        toggle_mode(context.world, states, links, pos, state);
        return BlockUseResult::Success;
    }
    if held == Some(LINKED_CONTROLLER) {
        return BlockUseResult::TryEmptyHand;
    }
    for first in [false, true] {
        if !slot_hit(state.facing, first, context.hit) {
            continue;
        }
        let Some(link) = links.get_mut(&pos) else {
            return BlockUseResult::Pass;
        };
        let previous = link.key.clone();
        if link.set_frequency(first, item.as_ref()) && link.key != previous {
            update_network(context.world, links, pos, &previous);
            let key = links.get(&pos).map(|link| link.key.clone());
            if let Some(key) = key {
                update_network(context.world, links, pos, &key);
            }
        }
        if let Some(effects) = EFFECTS.get() {
            effects.play_sound(
                context.world,
                [pos.x, pos.y, pos.z].map(|n| f64::from(n) + 0.5),
                "minecraft:entity.item_frame.add_item",
                SOUND_BLOCKS,
                0.25,
                0.1,
            );
        }
        return BlockUseResult::Success;
    }
    BlockUseResult::TryEmptyHand
}
