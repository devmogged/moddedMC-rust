//! Create 6.0.10's Andesite Funnel and Belt Funnel.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    effects::SOUND_BLOCKS,
    entities::{ENTITY_ALIVE, ENTITY_ITEM, Entity},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTAINERS, DIRECTIONS, EFFECTS, ENTITIES, STACKS, STONE, StateIds, WRENCH,
    belt_inventory::{self, Chain, TransportedItemStack, count},
    belt_movement::can_transport_objects,
    chute,
    kinetics::{FACES, Kind, Kinetics, Slope, face_axis, offset},
};

pub const ID: &str = "create:andesite_funnel";
pub const BELT_ID: &str = "create:andesite_belt_funnel";
const DOWN: u8 = 0;
const UP: u8 = 1;
const DEFAULT_EXTRACTION_TIMER: u32 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Retracted,
    Extended,
    Pushing,
    Pulling,
}

impl Shape {
    const fn name(self) -> &'static str {
        match self {
            Self::Retracted => "retracted",
            Self::Extended => "extended",
            Self::Pushing => "pushing",
            Self::Pulling => "pulling",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FunnelState {
    facing: u8,
    shape: Option<Shape>,
    extracting: bool,
    powered: bool,
    wet: bool,
}

impl FunnelState {
    fn all(belt: bool) -> Vec<Self> {
        let mut states = Vec::new();
        for facing in [2, 3, 4, 5, UP, DOWN] {
            let configs = if belt {
                if facing < 2 {
                    continue;
                }
                [
                    Shape::Retracted,
                    Shape::Extended,
                    Shape::Pushing,
                    Shape::Pulling,
                ]
                .map(|shape| (Some(shape), shape == Shape::Pushing))
                .to_vec()
            } else {
                vec![(None, false), (None, true)]
            };
            for (shape, extracting) in configs {
                for powered in [false, true] {
                    for wet in [false, true] {
                        states.push(Self {
                            facing,
                            shape,
                            extracting,
                            powered,
                            wet,
                        });
                    }
                }
            }
        }
        states
    }

    fn properties(self) -> String {
        let facing = DIRECTIONS[usize::from(self.facing)];
        let (powered, wet) = (self.powered, self.wet);
        if let Some(shape) = self.shape {
            format!(
                "facing={facing},powered={powered},shape={},waterlogged={wet}",
                shape.name()
            )
        } else {
            format!(
                "extracting={},facing={facing},powered={powered},waterlogged={wet}",
                self.extracting
            )
        }
    }

    const fn id(self) -> &'static str {
        if self.shape.is_some() { BELT_ID } else { ID }
    }

    fn state_id(self, states: &StateIds) -> Option<u32> {
        states.lookup(&format!("{}[{}]", self.id(), self.properties()))
    }

    fn model(self) -> String {
        let power = if self.powered { "powered" } else { "unpowered" };
        if let Some(shape) = self.shape {
            format!("create:block/andesite_belt_funnel_{}_{power}", shape.name())
        } else {
            let geometry = if self.facing >= 2 {
                "horizontal"
            } else {
                "vertical_filterless"
            };
            let mode = if self.extracting { "push" } else { "pull" };
            format!("create:block/andesite_funnel_{geometry}_{mode}_{power}")
        }
    }

    const fn rotation(self) -> (i16, i16) {
        let x = if self.facing == DOWN { 180 } else { 0 };
        let y = match self.facing {
            2 => 0,
            5 => 90,
            4 => 270,
            _ => 180,
        };
        (x, y)
    }

    fn storage(self, pos: Pos) -> Pos {
        shift(pos, self.facing ^ 1)
    }
}

fn bounds(state: FunnelState) -> Vec<ShapeBox> {
    // AllShapes uses tenths of a voxel on the sloped lip. ShapeBox uses whole voxels.
    let boxes: &[([i16; 3], [i16; 3])] = match (state.facing, state.shape) {
        (UP, _) => &[
            ([2, -2, 2], [14, 8, 14]),
            ([1, 1, 1], [15, 8, 15]),
            ([0, 4, 0], [16, 10, 16]),
        ],
        (DOWN, _) => &[
            ([2, 8, 2], [14, 18, 14]),
            ([1, 8, 1], [15, 15, 15]),
            ([0, 6, 0], [16, 12, 16]),
        ],
        (_, Some(Shape::Retracted)) => &[([2, -2, 14], [14, 14, 18]), ([0, -5, 8], [16, 16, 14])],
        (_, Some(Shape::Extended)) => &[
            ([2, -2, 14], [14, 14, 18]),
            ([3, -4, 10], [13, 13, 14]),
            ([2, -4, 6], [14, 14, 10]),
            ([0, -5, 0], [16, 16, 6]),
        ],
        (_, shape) => {
            let (base, lip) = if shape.is_some() { (-2, -3) } else { (2, -1) };
            return [
                ([2, base, 14], [14, 14, 18]),
                ([1, 8, 12], [15, 15, 14]),
                ([0, 13, 7], [16, 15, 11]),
                ([0, 9, 8], [16, 13, 12]),
                ([0, 5, 9], [16, 9, 13]),
                ([0, 1, 10], [16, 5, 14]),
                ([0, lip, 11], [16, 1, 15]),
            ]
            .into_iter()
            .map(|bounds| rotate_bounds(state.facing, bounds))
            .collect();
        }
    };
    boxes
        .iter()
        .map(|&bounds| rotate_bounds(state.facing, bounds))
        .collect()
}

fn rotate_bounds(face: u8, (min, max): ([i16; 3], [i16; 3])) -> ShapeBox {
    let [x, y, z] = min;
    let [xx, yy, zz] = max;
    let (min, max) = match face {
        3 => ([16 - xx, y, 16 - zz], [16 - x, yy, 16 - z]),
        4 => ([z, y, 16 - xx], [zz, yy, 16 - x]),
        5 => ([16 - zz, y, x], [16 - z, yy, xx]),
        _ => (min, max),
    };
    ShapeBox { min, max }
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for (belt, id, name) in [
        (false, ID, "Andesite Funnel"),
        (true, BELT_ID, "Andesite Belt Funnel"),
    ] {
        let rows: Vec<_> = FunnelState::all(belt)
            .into_iter()
            .map(|state| (state, state.properties(), state.model(), bounds(state)))
            .collect();
        let states: Vec<_> = rows
            .iter()
            .map(|(state, properties, model, shape)| {
                let (x, y) = state.rotation();
                State {
                    properties,
                    physical: STONE,
                    model,
                    rotating_model: "",
                    x,
                    y,
                    orientation: [0., 0., 0., 1.],
                    shape,
                }
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: "create:funnel",
                item_model: "create:item/andesite_funnel",
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for state in FunnelState::all(false)
        .into_iter()
        .chain(FunnelState::all(true))
    {
        states.add(content, state.id(), &state.properties(), None);
        if let Some(id) = state.state_id(states) {
            states.funnels.insert(id, state);
        }
    }
}

fn shift(pos: Pos, face: u8) -> Pos {
    offset(pos, FACES[usize::from(face)])
}

fn state_at(world: World<'_>, pos: Pos) -> u32 {
    let mut read = [content::UNLOADED_STATE];
    world.read(&[pos], &mut read);
    read[0]
}

fn shape_for_position(belt_facing: Option<u8>, facing: u8, extracting: bool) -> Shape {
    if belt_facing.is_some_and(|belt| face_axis(belt) == face_axis(facing)) {
        Shape::Retracted
    } else if extracting {
        Shape::Pushing
    } else {
        Shape::Pulling
    }
}

pub fn placed_state(args: Placement<'_>, default: FunnelState, states: &StateIds) -> Option<u32> {
    let facing = if default.shape.is_some() && args.face < 2 {
        [3, 4, 2, 5][((args.yaw / 90.0 + 0.5).floor() as i32 & 3) as usize]
    } else {
        args.face ^ 1
    };
    let wet = states.wet(state_at(args.world, args.position));
    let powered = args.world.has_neighbor_signal(args.position);
    let extracting = !args.sneak;
    let shape = default.shape.map(|_| {
        let belt = match states.kind(state_at(args.world, shift(args.position, DOWN))) {
            Some(Kind::Belt(belt)) => Some(belt.facing),
            _ => None,
        };
        shape_for_position(belt, facing, extracting)
    });
    let state = FunnelState {
        facing,
        powered,
        wet,
        extracting,
        shape,
    };
    state.state_id(states)
}

#[derive(Default)]
pub struct Funnels {
    entries: HashMap<Pos, FunnelBlockEntity>,
    order: Vec<Pos>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct FunnelData {
    transfer_cooldown: u32,
}

struct FunnelBlockEntity {
    state: FunnelState,
    data: FunnelData,
    dirty: bool,
}

pub fn event(world: World<'_>, funnels: &mut Funnels, states: &StateIds, event: &ContentEvent) {
    let pos = event.position;
    if let Some(&state) = states.funnels.get(&event.state)
        && event.reason != content::UNLOADED
    {
        if let Some(funnel) = funnels.entries.get_mut(&pos) {
            funnel.state = state;
        } else {
            let data = world
                .entity(pos)
                .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
                .unwrap_or_default();
            funnels.entries.insert(
                pos,
                FunnelBlockEntity {
                    state,
                    data,
                    dirty: false,
                },
            );
            funnels.order.push(pos);
        }
    } else if funnels.entries.remove(&pos).is_some() {
        funnels.order.retain(|other| *other != pos);
    }
}

/// Resolve the real block variant and POWERED before belts inspect the mouths.
pub fn update_states(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics) {
    for &pos in &kinetics.funnels.order {
        let Some(funnel) = kinetics.funnels.entries.get_mut(&pos) else {
            continue;
        };
        let mut state = funnel.state;
        state.powered = world.has_neighbor_signal(pos);
        let below = shift(pos, DOWN);
        let belt = match states.kind(state_at(world, below)) {
            Some(Kind::Belt(belt)) if can_transport_objects(belt) => Some(belt.facing),
            _ => None,
        };
        let valid = belt.is_some() || kinetics.depots.contains_key(&below);
        if state.facing >= 2 && valid {
            let desired = shape_for_position(belt, state.facing, state.extracting);
            state.shape = match (desired, state.shape) {
                (Shape::Retracted, Some(Shape::Extended)) => Some(Shape::Extended),
                (
                    Shape::Pushing | Shape::Pulling,
                    Some(current @ (Shape::Pushing | Shape::Pulling)),
                ) => Some(current),
                _ => Some(desired),
            };
        } else if let Some(shape) = state.shape.take() {
            state.extracting = shape == Shape::Pushing;
        }
        if state != funnel.state
            && let Some(id) = state.state_id(states)
            && world.write(&[BlockUpdate {
                position: pos,
                state: id,
                flags: content::UPDATE_CLIENTS,
            }]) == 1
        {
            funnel.state = state;
        }
    }
}

pub fn insert(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    face: u8,
    item: &TransportedItemStack,
    simulate: bool,
) -> i32 {
    if let Some(depot) = kinetics.depots.get_mut(&pos) {
        return depot.insert_item(world, pos, item, simulate);
    }
    if let Some(chute) = kinetics.chutes.entries.get_mut(&pos) {
        return chute.insert(item, 1.0, simulate);
    }
    if matches!(kinetics.kind(pos), Some(Kind::Belt(_))) {
        return belt_inventory::insert_into_segment(kinetics, pos, item, simulate);
    }
    let Some(container) = CONTAINERS
        .get()
        .and_then(|containers| containers.at(world, pos, face))
    else {
        return 0;
    };
    item.bytes()
        .and_then(|bytes| container.insert_stacked(&bytes, simulate))
        .map_or(0, |bytes| count(&item.item) - stack_count(&bytes))
}

fn extract(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    face: u8,
    amount: i32,
    simulate: bool,
) -> Option<Value> {
    if let Some(depot) = kinetics.depots.get_mut(&pos) {
        return depot.extract_items(world, amount, simulate);
    }
    if let Some(chute) = kinetics.chutes.entries.get_mut(&pos) {
        return chute.extract(world, amount, simulate);
    }
    if matches!(kinetics.kind(pos), Some(Kind::Belt(_))) {
        return belt_inventory::extract_from_segment(world, kinetics, pos, amount, simulate);
    }
    let container = CONTAINERS.get()?.at(world, pos, face)?;
    chute::extract_container(container, amount as u32, simulate)
}

fn stack_count(bytes: &[u8]) -> i32 {
    fastnbt::from_bytes::<Value>(bytes).map_or(0, |item| count(&item))
}

enum Mode {
    Paused,
    Collect,
    Extract,
    TakingFromBelt,
    PushingToBelt,
}

fn mode(state: FunnelState, kinetics: &Kinetics, pos: Pos) -> Mode {
    if state.powered {
        return Mode::Paused;
    }
    match state.shape {
        None if state.extracting => Mode::Extract,
        None => Mode::Collect,
        Some(Shape::Pulling) => Mode::TakingFromBelt,
        Some(Shape::Pushing) => Mode::PushingToBelt,
        Some(_) if belt_inventory::facing_at(kinetics, shift(pos, DOWN)) == Some(state.facing) => {
            Mode::PushingToBelt
        }
        Some(_) => Mode::TakingFromBelt,
    }
}

fn flap(world: World<'_>, pos: Pos, state: FunnelState) {
    if state.facing < 2 {
        return;
    }
    if let Some(&effects) = EFFECTS.get() {
        let (pos, _) = output_location(pos, state);
        effects.play_sound(
            world,
            pos,
            "minecraft:entity.item_frame.rotate_item",
            SOUND_BLOCKS,
            0.125,
            1.5,
        );
        effects.play_sound(
            world,
            pos,
            "minecraft:block.wool.break",
            SOUND_BLOCKS,
            0.0425,
            0.75,
        );
    }
}

fn output_location(pos: Pos, state: FunnelState) -> ([f64; 3], [f64; 3]) {
    let [dx, dy, dz] = FACES[usize::from(state.facing)].map(f64::from);
    let extent = match state.facing {
        DOWN => 0.5,
        UP => f64::from(0.15f32),
        _ => 0.25,
    };
    (
        [
            f64::from(pos.x) + 0.5 + dx * extent,
            f64::from(pos.y) + 0.5 + dy * extent
                - if state.facing >= 2 {
                    f64::from(0.45f32)
                } else {
                    0.0
                },
            f64::from(pos.z) + 0.5 + dz * extent,
        ],
        [0., if state.facing == UP { 0.25 } else { 0. }, 0.],
    )
}

fn activate_extractor(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    state: FunnelState,
) -> bool {
    let source = state.storage(pos);
    if extract(world, kinetics, source, state.facing, 1, true).is_none() {
        return false;
    }
    let Some(&entities) = ENTITIES.get() else {
        return false;
    };
    let [x, y, z] = [pos.x, pos.y, pos.z].map(f64::from);
    let low = if state.facing == UP { y } else { y - 1.0 };
    if entities
        .entities_in(world, [x, low, z], [x + 1., y + 1., z + 1.], 0)
        .into_iter()
        .any(|id| {
            entities
                .with_entity(world, id, |entity| {
                    entity
                        .view()
                        .is_some_and(|view| view.kind & ENTITY_ITEM != 0)
                        && entity.flags() & ENTITY_ALIVE != 0
                })
                .unwrap_or(false)
        })
    {
        return false;
    }
    let Some(item) = extract(world, kinetics, source, state.facing, 1, false) else {
        return false;
    };
    flap(world, pos, state);
    let (position, motion) = output_location(pos, state);
    if let (Some(&stacks), Ok(bytes)) = (STACKS.get(), fastnbt::to_bytes(&item)) {
        stacks.spawn(world, position, motion, &bytes);
    }
    true
}

fn insert_below(
    world: World<'_>,
    kinetics: &mut Kinetics,
    below: Pos,
    item: &TransportedItemStack,
    face: u8,
    simulate: bool,
) -> i32 {
    if let Some(depot) = kinetics.depots.get_mut(&below) {
        if simulate {
            return if depot.is_occupied() {
                0
            } else {
                count(&item.item).min(item.max_count())
            };
        }
        return crate::depot::try_inserting_from_side(world, below, depot, item, face);
    }
    if belt_inventory::try_inserting_from_side(kinetics, None, below, item, face, simulate) {
        count(&item.item)
    } else {
        0
    }
}

fn activate_extracting_belt_funnel(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    state: FunnelState,
) -> bool {
    let below = shift(pos, DOWN);
    let source = state.storage(pos);
    let Some(item) = extract(world, kinetics, source, state.facing, 1, true) else {
        return false;
    };
    let item = TransportedItemStack::new(item);
    if insert_below(world, kinetics, below, &item, state.facing, true) != count(&item.item) {
        return false;
    }
    let Some(item) = extract(world, kinetics, source, state.facing, 1, false) else {
        return false;
    };
    let item = TransportedItemStack::new(item);
    let accepted = insert_below(world, kinetics, below, &item, state.facing, false);
    if accepted == 0 {
        return false;
    }
    flap(world, pos, state);
    true
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    let order = std::mem::take(&mut kinetics.funnels.order);
    for &pos in &order {
        let Some(mut funnel) = kinetics.funnels.entries.remove(&pos) else {
            continue;
        };
        let mode = mode(funnel.state, kinetics, pos);
        let cooldown = funnel.data.transfer_cooldown;
        if matches!(mode, Mode::Paused) {
            funnel.data.transfer_cooldown = 0;
        } else if !matches!(mode, Mode::TakingFromBelt) {
            let ent = match mode {
                _ if cooldown > 0 => {
                    funnel.data.transfer_cooldown -= 1;
                    false
                }
                Mode::Extract => activate_extractor(world, kinetics, pos, funnel.state),
                Mode::PushingToBelt => {
                    activate_extracting_belt_funnel(world, kinetics, pos, funnel.state)
                }
                _ => false,
            };
            if ent {
                funnel.data.transfer_cooldown = DEFAULT_EXTRACTION_TIMER;
            }
        }
        funnel.dirty |= funnel.data.transfer_cooldown != cooldown;
        if funnel.dirty
            && let Ok(bytes) = fastnbt::to_bytes(&funnel.data)
            && world.set_entity(pos, &bytes)
        {
            funnel.dirty = false;
        }
        kinetics.funnels.entries.insert(pos, funnel);
    }
    kinetics.funnels.order = order;
}

fn try_insert(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    state: FunnelState,
    item: &TransportedItemStack,
) -> i32 {
    if state.powered {
        return 0;
    }
    let amount = insert(
        world,
        kinetics,
        state.storage(pos),
        state.facing,
        item,
        false,
    );
    if amount > 0 {
        flap(world, pos, state);
    }
    amount
}

pub fn direct_input_present(funnels: &Funnels, pos: Pos) -> bool {
    funnels
        .entries
        .get(&pos)
        .is_some_and(|funnel| funnel.state.shape.is_none())
}

pub fn insert_from_belt_end(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
) -> i32 {
    let Some(state) = kinetics
        .funnels
        .entries
        .get(&pos)
        .map(|funnel| funnel.state)
    else {
        return 0;
    };
    if state.shape.is_some() || state.extracting || state.facing != UP {
        return 0;
    }
    try_insert(world, kinetics, pos, state, item)
}

pub fn blocks_chute(funnels: &Funnels, pos: Pos) -> bool {
    funnels
        .entries
        .get(&pos)
        .is_some_and(|funnel| funnel.state.shape.is_none() && funnel.state.facing == DOWN)
}

pub fn check_for_funnels(
    world: World<'_>,
    kinetics: &mut Kinetics,
    chain: Chain,
    item: &mut TransportedItemStack,
    positive: bool,
    next: f32,
) -> (bool, bool) {
    let start = (item.pos.floor() as i32).clamp(0, chain.length - 1);
    let mut segment = start;
    let mut changed = false;
    let facing = chain.movement_facing();
    while (0..chain.length).contains(&segment)
        && if positive {
            segment as f32 <= next
        } else {
            (segment + 1) as f32 >= next
        }
    {
        let pos = shift(chain.position_for_offset(segment), UP);
        let offset = segment;
        segment += if positive { 1 } else { -1 };
        let Some(state) = kinetics
            .funnels
            .entries
            .get(&pos)
            .map(|funnel| funnel.state)
        else {
            continue;
        };
        let Some(shape) = state.shape else { continue };
        if state.facing == facing || shape == Shape::Pushing {
            continue;
        }
        let entry = funnel_entry(offset, shape, positive);
        if if positive {
            next <= entry
        } else {
            next >= entry
        } {
            return (false, changed);
        }
        let blocking = state.facing == facing ^ 1;
        if blocking {
            changed |= item.pos != entry;
            item.pos = entry;
        }
        let accepted = try_insert(world, kinetics, pos, state, item);
        if accepted > 0 {
            item.resize(world, count(&item.item) - accepted);
            changed = true;
        }
        if blocking || item.is_empty() {
            return (true, changed);
        }
    }
    (false, changed)
}

fn funnel_entry(offset: i32, shape: Shape, positive: bool) -> f32 {
    let shift = if shape == Shape::Extended {
        if positive { -0.499 } else { 0.499 }
    } else {
        0.0
    };
    offset as f32 + 0.5 + shift
}

pub fn pull_from_depots(world: World<'_>, kinetics: &mut Kinetics) {
    let positions: Vec<_> = kinetics
        .funnels
        .order
        .iter()
        .filter_map(|&pos| {
            let state = kinetics.funnels.entries.get(&pos)?.state;
            (state.shape == Some(Shape::Pulling)
                && !state.powered
                && kinetics.depots.contains_key(&shift(pos, DOWN)))
            .then_some((pos, state))
        })
        .collect();
    for (pos, state) in positions {
        let below = shift(pos, DOWN);
        let Some(mut depot) = kinetics.depots.remove(&below) else {
            continue;
        };
        depot.export_to_funnel(world, |item| try_insert(world, kinetics, pos, state, item));
        kinetics.depots.insert(below, depot);
    }
}

pub fn use_item_on(
    world: World<'_>,
    states: &StateIds,
    kinetics: &mut Kinetics,
    context: &mut UseBlock<'_>,
) -> BlockUseResult {
    let Some(&state) = states.funnels.get(&context.state) else {
        return BlockUseResult::Pass;
    };
    let Some(bytes) = context.stack.read() else {
        return BlockUseResult::TryEmptyHand;
    };
    let Ok(item) = fastnbt::from_bytes::<Value>(&bytes) else {
        return BlockUseResult::Fail;
    };
    let pos = context.position;
    if matches!(&item, Value::Compound(tag) if matches!(tag.get("id"), Some(Value::String(id)) if id == WRENCH))
    {
        let mut next = state;
        next.shape = match state.shape {
            None => {
                next.extracting = !state.extracting;
                None
            }
            Some(Shape::Pushing) => {
                next.extracting = false;
                Some(Shape::Pulling)
            }
            Some(Shape::Pulling) => {
                next.extracting = true;
                Some(Shape::Pushing)
            }
            Some(Shape::Extended) => Some(Shape::Retracted),
            Some(Shape::Retracted) if matches!(kinetics.kind(shift(pos, DOWN)), Some(Kind::Belt(belt)) if belt.slope != Slope::Horizontal) => {
                Some(Shape::Retracted)
            }
            Some(Shape::Retracted) => Some(Shape::Extended),
        };
        if let Some(id) = next.state_id(states)
            && next != state
        {
            world.write(&[BlockUpdate {
                position: pos,
                state: id,
                flags: content::UPDATE_ALL,
            }]);
        }
        return BlockUseResult::Success;
    }
    if state.shape.is_some() || state.extracting || state.powered || context.face != state.facing {
        return BlockUseResult::TryEmptyHand;
    }
    let item = TransportedItemStack::new(item);
    let accepted = try_insert(world, kinetics, pos, state, &item);
    if accepted > 0 {
        context.stack.shrink(accepted as u32);
    }
    BlockUseResult::Success
}

pub fn entity_inside(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    state: FunnelState,
    entity: Entity<'_>,
) {
    if state.shape.is_some() || state.extracting || state.powered {
        return;
    }
    let Some(view) = entity.view() else { return };
    if view.kind & ENTITY_ITEM == 0 || entity.flags() & ENTITY_ALIVE == 0 {
        return;
    }
    let normal = FACES[usize::from(state.facing)].map(f64::from);
    let origin = [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5);
    let projection: f64 = (0..3)
        .map(|axis| (view.position[axis] - origin[axis]) * normal[axis])
        .sum();
    if projection < -0.25 {
        return;
    }
    let Some(&stacks) = STACKS.get() else { return };
    let Some(item) = stacks
        .entity_stack(entity)
        .and_then(|nbt| fastnbt::from_bytes::<Value>(&nbt).ok())
    else {
        return;
    };
    let mut item = TransportedItemStack::new(item);
    let accepted = try_insert(world, kinetics, pos, state, &item);
    if accepted > 0 {
        let rest = count(&item.item) - accepted;
        if rest == 0 {
            stacks.set_entity_stack(entity, &[]);
        } else {
            item.resize(world, rest);
            if let Some(bytes) = item.bytes() {
                stacks.set_entity_stack(entity, &bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_is_on_the_approach_side_of_an_extended_funnel() {
        assert!((funnel_entry(3, Shape::Extended, true) - 3.001).abs() < 1e-6);
        assert!((funnel_entry(3, Shape::Extended, false) - 3.999).abs() < 1e-6);
        assert_eq!(funnel_entry(3, Shape::Retracted, true), 3.5);
    }
}
