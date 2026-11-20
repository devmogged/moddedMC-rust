//! Create 6.0.10's ChuteBlock and ChuteBlockEntity.

use std::collections::{HashMap, HashSet};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    entities::{ENTITY_ALIVE, ENTITY_ITEM},
    interaction::{BlockUseResult, UseBlock, UseOnBlock, UseResult},
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTAINERS, DIRECTIONS, DISPLAYS, ENTITIES, STACKS, StateIds, WRENCH,
    belt_inventory::{
        self, TransportedItemStack, count, item_display, remove_display, render_stack, with_count,
    },
    kinetics::{FACES, Kind, Kinetics, offset},
};

pub const ID: &str = "create:chute";
const DOWN: u8 = 0;
const UP: u8 = 1;
const HORIZONTAL: [u8; 4] = [3, 4, 2, 5];
const EXTRACTION_AMOUNT: u32 = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Normal,
    Window,
    Intersection,
    Encased,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ChuteState {
    facing: u8,
    shape: Shape,
    wet: bool,
}

impl ChuteState {
    fn all() -> impl Iterator<Item = Self> {
        [DOWN, 3, 4, 2, 5].into_iter().flat_map(|facing| {
            [
                Shape::Normal,
                Shape::Window,
                Shape::Intersection,
                Shape::Encased,
            ]
            .into_iter()
            .flat_map(move |shape| [false, true].map(|wet| Self { facing, shape, wet }))
        })
    }

    fn properties(self) -> String {
        let shape = match self.shape {
            Shape::Normal => "normal",
            Shape::Window => "window",
            Shape::Intersection => "intersection",
            Shape::Encased => "encased",
        };
        format!(
            "facing={},shape={shape},waterlogged={}",
            DIRECTIONS[usize::from(self.facing)],
            self.wet
        )
    }

    fn model(self) -> &'static str {
        match (self.facing == DOWN, self.shape) {
            (true, Shape::Normal) => "create:block/chute/block",
            (true, Shape::Window) => "create:block/chute/block_windowed",
            (true, _) => "create:block/chute/block_intersection",
            (false, Shape::Intersection) => "create:block/chute/block_diagonal_intersection",
            (false, Shape::Encased) => "create:block/chute/block_diagonal_encased",
            (false, _) => "create:block/chute/block_diagonal",
        }
    }

    const fn rotation(self) -> i16 {
        match self.facing {
            4 => 90,
            2 => 180,
            5 => 270,
            _ => 0,
        }
    }

    fn collision_shape(self) -> Vec<ShapeBox> {
        let cube = ShapeBox {
            min: [0; 3],
            max: [16; 3],
        };
        let intersection = matches!(self.shape, Shape::Intersection | Shape::Encased);
        if intersection {
            return vec![cube];
        }
        if self.facing == DOWN {
            return vec![
                ShapeBox {
                    min: [1, 8, 1],
                    max: [15, 16, 15],
                },
                ShapeBox {
                    min: [2, 0, 2],
                    max: [14, 8, 14],
                },
            ];
        }
        let mut boxes = Vec::new();
        for i in 0..16 {
            let low = (i - 15).max(0);
            let high = i + 4;
            let (min, max) = match self.facing {
                2 => ([1, low, 15 - i], [15, high, 16 - i]),
                4 => ([15 - i, low, 1], [16 - i, high, 15]),
                5 => ([i, low, 1], [i + 1, high, 15]),
                _ => ([1, low, i], [15, high, i + 1]),
            };
            boxes.push(ShapeBox { min, max });
        }
        boxes
    }

    fn target(self, pos: Pos) -> Pos {
        let below = shift(pos, DOWN);
        if self.facing == DOWN {
            below
        } else {
            shift(below, self.facing ^ 1)
        }
    }

    const fn is_open(self) -> bool {
        self.facing == DOWN || matches!(self.shape, Shape::Intersection)
    }
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let rows: Vec<_> = ChuteState::all()
        .map(|state| (state, state.properties(), state.collision_shape()))
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(state, properties, shape)| State {
            properties,
            physical: "minecraft:iron_block",
            model: state.model(),
            rotating_model: "",
            x: 0,
            y: state.rotation(),
            orientation: [0., 0., 0., 1.],
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Chute",
            entity: ID,
            item_model: "create:item/chute",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for state in ChuteState::all() {
        let properties = state.properties();
        states.add(content, ID, &properties, None);
        if let Some(id) = states.lookup(&format!("{ID}[{properties}]")) {
            states.chutes.insert(id, state);
        }
    }
}

fn shift(pos: Pos, face: u8) -> Pos {
    offset(pos, FACES[usize::from(face)])
}

fn read(world: World<'_>, pos: Pos) -> u32 {
    let mut state = [content::UNLOADED_STATE];
    world.read(&[pos], &mut state);
    state[0]
}

fn update_chute_state(
    world: World<'_>,
    states: &StateIds,
    pos: Pos,
    mut state: ChuteState,
) -> ChuteState {
    if state.facing != DOWN && !states.chutes.contains_key(&read(world, state.target(pos))) {
        state.facing = DOWN;
        state.shape = Shape::Normal;
        return state;
    }
    let above = shift(pos, UP);
    let diagonals = HORIZONTAL.map(|face| {
        states
            .chutes
            .get(&read(world, shift(above, face)))
            .is_some_and(|chute| chute.facing == face)
    });
    let connections = diagonals.into_iter().filter(|&connected| connected).count();
    if state.facing == DOWN {
        state.shape = if connections != 0 {
            Shape::Intersection
        } else if state.shape == Shape::Intersection {
            Shape::Normal
        } else {
            state.shape
        };
    } else {
        let straight_input = connections == 1
            && HORIZONTAL
                .into_iter()
                .zip(diagonals)
                .any(|(face, connected)| face == state.facing && connected);
        let vertical_input = states
            .chutes
            .get(&read(world, above))
            .is_some_and(|above| above.facing == DOWN);
        state.shape = if straight_input && !vertical_input {
            if state.shape == Shape::Encased {
                Shape::Encased
            } else {
                Shape::Normal
            }
        } else {
            Shape::Intersection
        };
    }
    state
}

pub fn placed_state(args: Placement<'_>, states: &StateIds) -> Option<u32> {
    let mut state = ChuteState {
        facing: DOWN,
        shape: Shape::Normal,
        wet: states.wet(read(args.world, args.position)),
    };
    if args.face >= 2 && !args.sneak {
        state.facing = args.face;
        state = update_chute_state(args.world, states, args.position, state);
    }
    states.lookup(&format!("{ID}[{}]", state.properties()))
}

/// `ChuteItem.place` connects to the side one level up.
pub fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    let Some(states) = crate::states() else {
        return UseResult::Pass;
    };
    if context.sneaking
        || !(2..=5).contains(&context.face)
        || !states
            .chutes
            .contains_key(&read(context.world, context.position))
    {
        return UseResult::Pass;
    }
    let world = context.world;
    let position = shift(shift(context.position, context.face), UP);
    let old = read(world, position);
    let replaced = states.replaceable.contains(&old);
    let mut state = if replaced {
        ChuteState {
            facing: DOWN,
            shape: Shape::Normal,
            wet: states.wet(old),
        }
    } else if let Some(&state) = states.chutes.get(&old)
        && state.facing == DOWN
    {
        state
    } else {
        return UseResult::Fail;
    };
    state.facing = context.face;
    let state = update_chute_state(world, states, position, state);
    let Some(state) = states.lookup(&format!("{ID}[{}]", state.properties())) else {
        return UseResult::Fail;
    };
    if world.write(&[BlockUpdate {
        position,
        state,
        flags: content::UPDATE_ALL,
    }]) != 1
    {
        return UseResult::Fail;
    }
    if replaced && !context.creative {
        context.stack.shrink(1);
    }
    UseResult::Success
}

#[derive(Default)]
pub struct Chutes {
    pub entries: HashMap<Pos, ChuteBlockEntity>,
    order: Vec<Pos>,
    shape_updates: HashSet<Pos>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct ChuteData {
    item: Value,
    item_position: f32,
    pull: f32,
    push: f32,
    bottom_air_flow_distance: f32,
}

impl Default for ChuteData {
    fn default() -> Self {
        Self {
            item: Value::Compound(HashMap::new()),
            item_position: 0.0,
            pull: 0.0,
            push: 0.0,
            bottom_air_flow_distance: 0.0,
        }
    }
}

pub struct ChuteBlockEntity {
    state: ChuteState,
    item: Option<TransportedItemStack>,
    item_position: f32,
    dirty: bool,
}

impl ChuteBlockEntity {
    fn load(world: World<'_>, pos: Pos, state: ChuteState) -> Self {
        let data: ChuteData = world
            .entity(pos)
            .and_then(|nbt| fastnbt::from_bytes(&nbt).ok())
            .unwrap_or_default();
        let item = nonempty(&data.item).then(|| TransportedItemStack::new(data.item));
        Self {
            state,
            item,
            item_position: data.item_position,
            dirty: false,
        }
    }

    pub fn insert(&mut self, stack: &TransportedItemStack, position: f32, simulate: bool) -> i32 {
        if self.item.is_some() {
            return 0;
        }
        let inserted = stack.max_count().min(count(&stack.item));
        if inserted > 0 && !simulate {
            self.item = Some(TransportedItemStack::new(with_count(&stack.item, inserted)));
            self.item_position = position;
            self.dirty = true;
        }
        inserted
    }

    pub fn extract(&mut self, world: World<'_>, limit: i32, simulate: bool) -> Option<Value> {
        let item = self.item.as_mut()?;
        let count = count(&item.item);
        let amount = count.min(limit);
        if amount <= 0 {
            return None;
        }
        let extracted = with_count(&item.item, amount);
        if !simulate {
            if amount == count {
                remove_display(world, item);
                self.item = None;
            } else {
                item.resize(world, count - amount);
            }
            self.item_position = 1.0;
            self.dirty = true;
        }
        Some(extracted)
    }

    fn remove(self, world: World<'_>, pos: Pos, drop: bool) {
        if let Some(item) = self.item {
            remove_display(world, &item);
            if drop && let (Some(&stacks), Some(bytes)) = (STACKS.get(), item.bytes()) {
                stacks.spawn(world, center(pos, 0.5), [0.0; 3], &bytes);
            }
        }
    }
}

fn nonempty(item: &Value) -> bool {
    matches!(item, Value::Compound(fields) if fields.contains_key("id")) && count(item) > 0
}

pub fn event(world: World<'_>, chutes: &mut Chutes, states: &StateIds, event: &ContentEvent) {
    let pos = event.position;
    let before = states.chutes.get(&event.previous).copied();
    let after = states.chutes.get(&event.state).copied();
    if before.is_none() && after.is_none() && !chutes.entries.contains_key(&pos) {
        return;
    }
    if event.reason == content::UNLOADED || after.is_none() {
        if let Some(old) = chutes.entries.remove(&pos) {
            old.remove(world, pos, event.reason != content::UNLOADED);
            chutes.order.retain(|other| *other != pos);
        }
    } else if let Some(state) = after {
        if let Some(chute) = chutes.entries.get_mut(&pos) {
            chute.state = state;
        } else {
            chutes.order.push(pos);
            chutes
                .entries
                .insert(pos, ChuteBlockEntity::load(world, pos, state));
        }
        chutes.shape_updates.insert(pos);
    }
    for state in [before, after].into_iter().flatten() {
        chutes.shape_updates.insert(state.target(pos));
    }
    for face in HORIZONTAL {
        chutes.shape_updates.insert(shift(shift(pos, UP), face));
    }
}

pub fn direct_insert(
    world: World<'_>,
    states: &StateIds,
    chutes: &mut Chutes,
    pos: Pos,
    item: &TransportedItemStack,
) -> i32 {
    let Some(chute) = chutes.entries.get_mut(&pos) else {
        return 0;
    };
    if !can_directly_insert(world, states, pos, chute.state) {
        return 0;
    }
    chute.insert(item, 1.0, false)
}

fn can_directly_insert(world: World<'_>, states: &StateIds, pos: Pos, state: ChuteState) -> bool {
    state.is_open()
        && !states
            .chutes
            .get(&read(world, shift(pos, UP)))
            .is_some_and(|above| above.facing == DOWN)
}

fn landing_item(world: World<'_>, states: &StateIds, pos: Pos, chute: &mut ChuteBlockEntity) {
    if !can_directly_insert(world, states, pos, chute.state) {
        return;
    }
    let (Some(&entities), Some(&stacks)) = (ENTITIES.get(), STACKS.get()) else {
        return;
    };
    let [x, y, z] = center(pos, 1.0);
    let min = [x - 0.5, y - 0.05, z - 0.5];
    let max = [x + 0.5, y + 0.05, z + 0.5];
    for id in entities.entities_in(world, min, max, 0) {
        let accepted = entities.with_entity(world, id, |entity| {
            let Some(view) = entity.view() else {
                return false;
            };
            if view.kind & ENTITY_ITEM == 0
                || entity.flags() & ENTITY_ALIVE == 0
                || !view.on_ground
                || view.motion[1] > 0.0
                || (view.position[1] - y).abs() > 0.0001
                || view.position[0].floor() != f64::from(pos.x)
                || view.position[2].floor() != f64::from(pos.z)
            {
                return false;
            }
            let Some(item) = stacks
                .entity_stack(entity)
                .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
            else {
                return false;
            };
            let item = TransportedItemStack::new(item);
            let inserted = chute.insert(&item, 1., true);
            if inserted == 0 {
                return false;
            }
            let rest = count(&item.item) - inserted;
            let mut remainder = Vec::new();
            if rest > 0 {
                let Ok(bytes) = fastnbt::to_bytes(&with_count(&item.item, rest)) else {
                    return false;
                };
                remainder = bytes;
            }
            if !stacks.set_entity_stack(entity, &remainder) {
                return false;
            }
            chute.insert(&item, 1., false) > 0
        });
        if accepted == Some(true) {
            break;
        }
    }
}

fn center(pos: Pos, y: f64) -> [f64; 3] {
    [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + y,
        f64::from(pos.z) + 0.5,
    ]
}

pub fn use_item_on(
    world: World<'_>,
    chutes: &mut Chutes,
    context: &mut UseBlock<'_>,
) -> BlockUseResult {
    if !context.with_item {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(chute) = chutes.entries.get_mut(&context.position) else {
        return BlockUseResult::TryEmptyHand;
    };
    if let Some(bytes) = context.stack.read() {
        let Some(Value::Compound(item)) = fastnbt::from_bytes::<Value>(&bytes).ok() else {
            return BlockUseResult::TryEmptyHand;
        };
        if !matches!(item.get("id"), Some(Value::String(id)) if id == WRENCH)
            || chute.state.shape == Shape::Intersection
        {
            return BlockUseResult::TryEmptyHand;
        }
        let shape = match chute.state.shape {
            Shape::Encased => Shape::Normal,
            Shape::Normal if chute.state.facing == DOWN => Shape::Window,
            Shape::Window if chute.state.facing == DOWN => Shape::Normal,
            _ => return BlockUseResult::Success,
        };
        let next = ChuteState {
            shape,
            ..chute.state
        };
        if let Some(id) = crate::states()
            .and_then(|states| states.lookup(&format!("{ID}[{}]", next.properties())))
            && world.write(&[BlockUpdate {
                position: context.position,
                state: id,
                flags: content::UPDATE_ALL,
            }]) == 1
        {
            chute.state = next;
        }
        return BlockUseResult::Success;
    }
    let Some(item) = &chute.item else {
        return BlockUseResult::TryEmptyHand;
    };
    if !item.bytes().is_some_and(|bytes| context.give(&bytes)) {
        return BlockUseResult::Fail;
    }
    remove_display(world, item);
    chute.item = None;
    chute.item_position = 1.0;
    chute.dirty = true;
    BlockUseResult::Success
}

/// `ChuteBlockEntity.handleDownwardOutput` returns how much the outlet can take.
fn handle_downward_output(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    state: ChuteState,
    item: &TransportedItemStack,
    simulate: bool,
) -> i32 {
    let below = shift(pos, DOWN);
    let amount = count(&item.item);
    let via_inventory = if matches!(kinetics.kind(below), Some(Kind::Belt(_))) {
        Some(belt_inventory::insert_into_segment(
            kinetics, below, item, simulate,
        ))
    } else if let Some(depot) = kinetics.depots.get_mut(&below) {
        Some(depot.insert_item(world, below, item, simulate))
    } else if let Some(container) = CONTAINERS
        .get()
        .and_then(|containers| containers.at(world, below, UP))
    {
        let insert = item
            .bytes()
            .and_then(|bytes| container.insert_stacked(&bytes, simulate));
        Some(insert.map_or(0, |bytes| amount - stack_count(&bytes)))
    } else {
        None
    };
    if let Some(accepted) = via_inventory
        && (accepted > 0 || state.facing == DOWN)
    {
        return accepted;
    }
    if let Some(target) = kinetics.chutes.entries.get_mut(&state.target(pos)) {
        let position = if state.facing == DOWN { 1.0 } else { 0.51 };
        return target.insert(item, position, simulate);
    }
    if state.facing != DOWN
        || read(world, below) == content::UNLOADED_STATE
        || world.face_sturdy(below, UP)
        || crate::funnel::blocks_chute(&kinetics.funnels, below)
    {
        return 0;
    }
    if simulate
        || item.bytes().is_some_and(|bytes| {
            STACKS
                .get()
                .and_then(|stacks| stacks.spawn(world, center(pos, -0.25), [0., -0.25, 0.], &bytes))
                .is_some()
        })
    {
        amount
    } else {
        0
    }
}

fn stack_count(bytes: &[u8]) -> i32 {
    fastnbt::from_bytes::<Value>(bytes).map_or(0, |item| count(&item))
}

fn extract_from_above(world: World<'_>, kinetics: &mut Kinetics, pos: Pos) -> Option<Value> {
    let source = shift(pos, UP);
    if let Some(depot) = kinetics.depots.get_mut(&source) {
        return depot.extract_items(world, EXTRACTION_AMOUNT as i32, false);
    }
    if matches!(kinetics.kind(source), Some(Kind::Belt(_))) {
        return belt_inventory::extract_from_segment(
            world,
            kinetics,
            source,
            EXTRACTION_AMOUNT as i32,
            false,
        );
    }
    let container = CONTAINERS.get()?.at(world, source, DOWN)?;
    extract_container(container, EXTRACTION_AMOUNT, false)
}

/// `ItemHelper.extract`, UPTO mode for an unfiltered handler.
pub fn extract_container(
    container: moddedmc_api::containers::Container<'_>,
    limit: u32,
    simulate: bool,
) -> Option<Value> {
    let mut extracting: Option<Value> = None;
    let mut total = 0;
    for slot in 0..container.slots()? {
        let Some(bytes) = container.extract(slot, limit - total, true) else {
            continue;
        };
        let Ok(item) = fastnbt::from_bytes::<Value>(&bytes) else {
            continue;
        };
        let amount = count(&item).max(0) as u32;
        let Some(info) = STACKS.get().and_then(|stacks| stacks.info(&bytes)) else {
            continue;
        };
        if amount == 0
            || extracting.as_ref().is_some_and(|prior| {
                with_count(prior, 1) != with_count(&item, 1) || total + amount > info.max_count
            })
        {
            continue;
        }
        let taken = if simulate {
            amount
        } else {
            container
                .extract(slot, amount, false)
                .map_or(0, |bytes| stack_count(&bytes).max(0) as u32)
        };
        if taken == 0 {
            continue;
        }
        total += taken;
        extracting = Some(with_count(&item, total as i32));
        if total >= limit {
            break;
        }
    }
    extracting
}

fn advance(position: f32, output_ready: bool) -> f32 {
    const GRAVITY: f32 = 4.0 / 20.0;
    let next = position - GRAVITY;
    if next < 0.5 && !output_ready {
        0.5
    } else {
        next
    }
}

fn tick_items(
    world: World<'_>,
    states: &StateIds,
    kinetics: &mut Kinetics,
    pos: Pos,
    chute: &mut ChuteBlockEntity,
) {
    let Some(item) = &mut chute.item else {
        if let Some(item) = extract_from_above(world, kinetics, pos) {
            chute.item = Some(TransportedItemStack::new(item));
            chute.item_position = 1.;
            chute.dirty = true;
        } else {
            landing_item(world, states, pos, chute);
        }
        return;
    };
    let old = chute.item_position;
    let can_output = advance(old, true) >= 0.5
        || handle_downward_output(world, kinetics, pos, chute.state, item, true) > 0;
    let next = advance(old, can_output);
    if next < 0.0 {
        let accepted = handle_downward_output(world, kinetics, pos, chute.state, item, false);
        let rest = count(&item.item) - accepted;
        if accepted > 0 {
            if rest == 0 {
                remove_display(world, item);
                chute.item = None;
                chute.item_position = 1.0;
            } else {
                item.resize(world, rest);
            }
            chute.dirty = true;
        }
    } else if next != old {
        chute.item_position = next;
        chute.dirty = true;
    }
}

fn flush(world: World<'_>, pos: Pos, chute: &mut ChuteBlockEntity) {
    if let Some(item) = &mut chute.item {
        if chute.state.facing == DOWN && chute.state.shape == Shape::Window {
            if let Some(&displays) = DISPLAYS.get() {
                let half_angle = chute.item_position * std::f32::consts::FRAC_PI_2;
                let (sin, cos) = half_angle.sin_cos();
                let mut display =
                    item_display(center(pos, f64::from(chute.item_position)), 0, true);
                display.rotation = [sin * cos, cos * sin, sin * sin, cos * cos];
                display.interpolation = 1;
                render_stack(world, displays, item, display);
            }
        } else {
            remove_display(world, item);
            item.clear_display();
        }
    }
    if chute.dirty {
        let mut data = ChuteData {
            item_position: chute.item_position,
            ..ChuteData::default()
        };
        if let Some(item) = &chute.item {
            data.item = item.item.clone();
        }
        if let Ok(bytes) = fastnbt::to_bytes(&data)
            && world.set_entity(pos, &bytes)
        {
            chute.dirty = false;
        }
    }
}

pub fn tick(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics) {
    for pos in kinetics.chutes.shape_updates.drain() {
        let Some(chute) = kinetics.chutes.entries.get_mut(&pos) else {
            continue;
        };
        let updated = update_chute_state(world, states, pos, chute.state);
        if updated != chute.state
            && let Some(id) = states.lookup(&format!("{ID}[{}]", updated.properties()))
            && world.write(&[BlockUpdate {
                position: pos,
                state: id,
                flags: content::UPDATE_ALL,
            }]) == 1
        {
            chute.state = updated;
        }
    }
    let order = std::mem::take(&mut kinetics.chutes.order);
    for &pos in &order {
        let Some(mut chute) = kinetics.chutes.entries.remove(&pos) else {
            continue;
        };
        tick_items(world, states, kinetics, pos, &mut chute);
        flush(world, pos, &mut chute);
        kinetics.chutes.entries.insert(pos, chute);
    }
    kinetics.chutes.order = order;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_items_resume_from_the_halfway_point() {
        let mut position = 1.0;
        for _ in 0..8 {
            position = advance(position, false);
        }
        assert_eq!(position, 0.5);
        position = advance(position, true);
        assert!((position - 0.3).abs() < f32::EPSILON);
        position = advance(position, true);
        assert!((position - 0.1).abs() < f32::EPSILON);
        assert!(advance(position, true) < 0.0);
    }
}
