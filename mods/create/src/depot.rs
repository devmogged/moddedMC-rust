//! Create 6.0.10's DepotBlock, DepotBlockEntity and DepotBehaviour.

use std::collections::{BTreeMap, HashMap};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, Content, ContentEvent, ShapeBox, State, World},
    effects::{SOUND_BLOCKS, SOUND_PLAYERS},
    entities::{ENTITY_ALIVE, Entity},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    DISPLAYS, EFFECTS, STACKS, STONE, StateIds,
    belt_inventory::{
        TransportedItemStack, count, item_display, next_int, remove_display, render_stack,
        with_count,
    },
    kinetics::{FACES, Kinetics, face_axis},
    press::{self, ProcessingResult},
};

pub const ID: &str = "create:depot";
const UP: u8 = 1;
const DOWN: u8 = 0;
const SURFACE: f64 = 13.0 / 16.0;
const OUTPUT_SLOTS: u8 = 8;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let states = ["waterlogged=false", "waterlogged=true"].map(|properties| State {
        properties,
        model: "create:block/depot/block",
        physical: STONE,
        rotating_model: "",
        x: 0,
        y: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
        shape: &[ShapeBox {
            min: [0, 0, 0],
            max: [16, 13, 16],
        }],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Depot",
            entity: ID,
            item_model: "create:item/depot",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn is_depot(states: &StateIds, id: u32) -> bool {
    states.by_id.get(&id).is_some_and(|state| state.block == ID)
}

#[derive(Default)]
pub struct DepotBehaviour {
    held_item: Option<TransportedItemStack>,
    processing_output_buffer: BTreeMap<u8, TransportedItemStack>,
    dirty: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DepotData {
    #[serde(skip_serializing_if = "Option::is_none")]
    held_item: Option<TransportedItemStack>,
    #[serde(default)]
    output_buffer: OutputBufferData,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OutputBufferData {
    size: i32,
    items: Vec<Value>,
}

impl Default for OutputBufferData {
    fn default() -> Self {
        Self {
            size: i32::from(OUTPUT_SLOTS),
            items: Vec::new(),
        }
    }
}

impl DepotBehaviour {
    fn load(world: World<'_>, pos: Pos) -> Self {
        world
            .entity(pos)
            .as_deref()
            .and_then(Self::decode)
            .unwrap_or_default()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let saved: DepotData = fastnbt::from_bytes(bytes).ok()?;
        let outputs = saved
            .output_buffer
            .items
            .into_iter()
            .filter_map(|mut item| {
                let Value::Compound(data) = &mut item else {
                    return None;
                };
                let Some(Value::Int(slot)) = data.remove("Slot") else {
                    return None;
                };
                let slot = u8::try_from(slot)
                    .ok()
                    .filter(|slot| *slot < OUTPUT_SLOTS)?;
                (count(&item) > 0).then(|| (slot, TransportedItemStack::new(item)))
            });
        Some(Self {
            held_item: saved.held_item.filter(|item| !item.is_empty()),
            processing_output_buffer: outputs.collect(),
            dirty: false,
        })
    }

    fn save(&self) -> Result<Vec<u8>, fastnbt::error::Error> {
        let items = self
            .processing_output_buffer
            .iter()
            .filter_map(|(slot, stack)| {
                let Value::Compound(mut item) = stack.item.clone() else {
                    return None;
                };
                item.insert("Slot".into(), Value::Int(i32::from(*slot)));
                Some(Value::Compound(item))
            });
        fastnbt::to_bytes(&DepotData {
            held_item: self.held_item.clone(),
            output_buffer: OutputBufferData {
                items: items.collect(),
                ..OutputBufferData::default()
            },
        })
    }

    pub fn is_occupied(&self) -> bool {
        self.held_item.as_ref().is_some_and(|item| !item.is_empty())
            || !self.processing_output_buffer.is_empty()
    }

    // TODO(abi): analog output
    #[allow(
        dead_code,
        reason = "the content ABI cannot advertise analog output yet"
    )]
    fn get_analog_output_signal(&self) -> u8 {
        let held_count = self.held_item.as_ref().map_or(0, |held| count(&held.item));
        let output_count: i32 = self
            .processing_output_buffer
            .values()
            .map(|slot| count(&slot.item))
            .sum();
        let max = self
            .held_item
            .as_ref()
            .and_then(|held| STACKS.get()?.info(&held.bytes()?))
            .map_or(64, |info| {
                if info.max_count == 0 {
                    64
                } else {
                    info.max_count
                }
            });
        let fill = (held_count + output_count) as f32 / max as f32;
        ((fill * 14.0).floor() as i32 + i32::from(fill > 0.0)).clamp(0, 15) as u8
    }

    /// DirectBeltInputBehaviour sets the animation origin before passing it to insert.
    fn insert_from_side(&mut self, stack: &TransportedItemStack, side: u8, max: i32) -> i32 {
        if self.is_occupied() {
            return 0;
        }
        let offered = count(&stack.item);
        let inserted = offered.min(max);
        if inserted <= 0 {
            return 0;
        }
        let mut stack = stack.clone();
        stack.pos = if face_axis(side) == 1 { 0.5 } else { 0.0 };
        stack.prev_pos = stack.pos;
        stack.prev_offset = stack.offset;
        stack.in_direction = i32::from(side);
        stack.locked = false;
        stack.locked_externally = false;
        if offered != inserted {
            stack.item = with_count(&stack.item, inserted);
            stack.clear_display();
        }
        self.held_item = Some(stack);
        self.dirty = true;
        inserted
    }

    /// `DepotItemHandler.insertItem` starts with a fresh transported stack.
    pub fn insert_item(
        &mut self,
        world: World<'_>,
        pos: Pos,
        stack: &TransportedItemStack,
        simulate: bool,
    ) -> i32 {
        if self.is_occupied() {
            return 0;
        }
        let max = stack.max_count();
        let accepted = count(&stack.item).min(max);
        if accepted > 0 && !simulate {
            let mut item = TransportedItemStack::new(with_count(&stack.item, accepted));
            item.pos = 0.0;
            self.held_item = Some(item);
            self.dirty = true;
            play_insert(world, pos, UP);
        }
        accepted
    }

    /// `ItemHelper.extract` from the held slot and then compatible output slots.
    pub fn extract_items(&mut self, world: World<'_>, limit: i32, simulate: bool) -> Option<Value> {
        let mut result = None;
        if let Some(item) = &mut self.held_item
            && extract_matching(world, item, &mut result, limit, simulate)
        {
            self.held_item = None;
        }
        self.processing_output_buffer
            .retain(|_, item| !extract_matching(world, item, &mut result, limit, simulate));
        self.dirty |= !simulate && result.is_some();
        result
    }

    /// `DepotBehaviour.handleBeltFunnelOutput`: output slots first, then the held item.
    pub fn export_to_funnel(
        &mut self,
        world: World<'_>,
        mut insert: impl FnMut(&TransportedItemStack) -> i32,
    ) {
        let Some(held) = &mut self.held_item else {
            return;
        };
        if 0.5 - held.pos >= 1.0 / 16.0 {
            return;
        }
        let mut sent = false;
        self.processing_output_buffer.retain(|_, item| {
            if sent {
                return true;
            }
            let (changed, keep) = export_item(world, item, &mut insert);
            sent = changed;
            keep
        });
        if !sent {
            let (changed, keep) = export_item(world, held, &mut insert);
            sent = changed;
            if !keep {
                self.held_item = None;
            }
        }
        self.dirty |= sent;
    }

    fn remove(self, world: World<'_>, pos: Pos, drop: bool) {
        for stack in self
            .held_item
            .into_iter()
            .chain(self.processing_output_buffer.into_values())
        {
            remove_display(world, &stack);
            if drop && let (Some(&stacks), Some(bytes)) = (STACKS.get(), stack.bytes()) {
                stacks.spawn(world, center(pos, 1.0), [0.0; 3], &bytes);
            }
        }
    }
}

fn extract_matching(
    world: World<'_>,
    stack: &mut TransportedItemStack,
    result: &mut Option<Value>,
    limit: i32,
    simulate: bool,
) -> bool {
    let total = result.as_ref().map_or(0, count);
    let amount = count(&stack.item);
    let max = stack.max_count();
    let taken = (limit - total).min(amount).min(max);
    if taken <= 0
        || result.as_ref().is_some_and(|result| {
            with_count(result, 1) != with_count(&stack.item, 1) || total + taken > max
        })
    {
        return false;
    }
    *result = Some(with_count(&stack.item, total + taken));
    if simulate {
        return false;
    }
    if taken == amount {
        remove_display(world, stack);
        true
    } else {
        stack.resize(world, amount - taken);
        false
    }
}

fn export_item(
    world: World<'_>,
    item: &mut TransportedItemStack,
    insert: &mut impl FnMut(&TransportedItemStack) -> i32,
) -> (bool, bool) {
    let sent = insert(item);
    if sent <= 0 {
        return (false, true);
    }
    let rest = count(&item.item) - sent;
    if rest == 0 {
        remove_display(world, item);
        (true, false)
    } else {
        item.resize(world, rest);
        (true, true)
    }
}

pub fn event(
    world: World<'_>,
    depots: &mut HashMap<Pos, DepotBehaviour>,
    states: &StateIds,
    event: &ContentEvent,
) {
    let pos = event.position;
    let after = is_depot(states, event.state);
    match event.reason {
        content::LOADED if after => {
            depots
                .entry(pos)
                .or_insert_with(|| DepotBehaviour::load(world, pos));
        }
        content::UNLOADED => {
            if let Some(depot) = depots.remove(&pos) {
                depot.remove(world, pos, false);
            }
        }
        content::LOADED | content::NEIGHBOR_CHANGED => {}
        _ if after => {
            depots.entry(pos).or_insert_with(|| DepotBehaviour {
                dirty: true,
                ..DepotBehaviour::default()
            });
        }
        _ if is_depot(states, event.previous) => {
            if let Some(depot) = depots.remove(&pos) {
                depot.remove(world, pos, true);
            }
        }
        _ => {}
    }
}

pub fn try_inserting_from_side(
    world: World<'_>,
    pos: Pos,
    depot: &mut DepotBehaviour,
    item: &TransportedItemStack,
    side: u8,
) -> i32 {
    if depot.is_occupied() {
        return 0;
    }
    let max = item.max_count();
    let inserted = depot.insert_from_side(item, side, max);
    if inserted > 0 {
        play_insert(world, pos, side);
    }
    inserted
}

fn play_insert(world: World<'_>, pos: Pos, side: u8) {
    if let Some(&effects) = EFFECTS.get() {
        let (sound, volume, pitch) = if face_axis(side) == 1 {
            ("minecraft:entity.item_frame.add_item", 0.25, 1.25)
        } else {
            ("minecraft:block.sand.break", 0.125, 1.5)
        };
        effects.play_sound(world, center(pos, 0.5), sound, SOUND_BLOCKS, volume, pitch);
    }
}

/// SharedDepotBlockMethods.onUse. The held stack is returned before the output slots.
pub fn use_item_on(
    depots: &mut HashMap<Pos, DepotBehaviour>,
    context: &mut UseBlock<'_>,
) -> BlockUseResult {
    if context.face != UP {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(player) = context.player else {
        return BlockUseResult::Pass;
    };
    let pos = context.position;
    let world = context.world;
    let depot = depots
        .entry(pos)
        .or_insert_with(|| DepotBehaviour::load(world, pos));
    let was_empty_handed = context.stack.read().is_none();
    if let Some(item) = &depot.held_item {
        if !item.bytes().is_some_and(|bytes| context.give(&bytes)) {
            return BlockUseResult::Fail;
        }
        remove_display(world, item);
        depot.held_item = None;
        depot.dirty = true;
        if let Some(&effects) = EFFECTS.get() {
            let pitch = 1.0 + next_int(1 << 24) as f32 / (1 << 24) as f32;
            effects.play_sound(
                world,
                center(pos, 0.5),
                "minecraft:entity.item.pickup",
                SOUND_PLAYERS,
                0.2,
                pitch,
            );
        }
    }
    depot.processing_output_buffer.retain(|_, item| {
        if item.bytes().is_some_and(|bytes| context.give(&bytes)) {
            remove_display(world, item);
            depot.dirty = true;
            false
        } else {
            true
        }
    });
    if !was_empty_handed {
        let Some(item) = context
            .stack
            .read()
            .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
        else {
            return BlockUseResult::Fail;
        };
        if matches!(&item, Value::Compound(tag) if tag.get("id") == Some(&Value::String("create:mechanical_arm".into())))
        {
            return BlockUseResult::Success;
        }
        if !context.stack.write(&[]) {
            return BlockUseResult::Fail;
        }
        let mut item = TransportedItemStack::new(item);
        let yaw = player.view().map_or(0.0, |view| view.yaw);
        let from = [3, 4, 2, 5][((yaw / 90.0 + 0.5).floor() as i32 & 3) as usize];
        item.in_direction = i32::from(from);
        item.pos = 0.25;
        item.prev_pos = item.pos;
        depot.held_item = Some(item);
        depot.dirty = true;
        play_insert(world, pos, from);
    }
    BlockUseResult::Success
}

/// SharedDepotBlockMethods.onLanded for item entities whose feet reached the depot top.
pub fn on_landed(world: World<'_>, pos: Pos, depot: &mut DepotBehaviour, entity: Entity<'_>) {
    let Some(view) = entity.view() else { return };
    if depot.is_occupied()
        || entity.flags() & ENTITY_ALIVE == 0
        || !view.on_ground
        || view.motion[1] > 0.0
        || (view.position[1] - f64::from(pos.y) - SURFACE).abs() > 1e-5
        || view.position[0].floor() != f64::from(pos.x)
        || view.position[2].floor() != f64::from(pos.z)
    {
        return;
    }
    let Some(&stacks) = STACKS.get() else { return };
    let Some(item) = stacks
        .entity_stack(entity)
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
    else {
        return;
    };
    let item = TransportedItemStack::new(item);
    let accepted = try_inserting_from_side(world, pos, depot, &item, DOWN);
    let remaining = count(&item.item) - accepted;
    if accepted > 0 {
        let bytes = if remaining == 0 {
            Vec::new()
        } else if let Ok(bytes) = fastnbt::to_bytes(&with_count(&item.item, remaining)) {
            bytes
        } else {
            depot.held_item = None;
            return;
        };
        if !stacks.set_entity_stack(entity, &bytes) {
            depot.held_item = None;
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

/// DepotBehaviour.tick(TransportedItemStack).
fn slide(item: &mut TransportedItemStack) -> bool {
    item.prev_pos = item.pos;
    item.prev_offset = item.offset;
    let diff = 0.5 - item.pos;
    if diff <= 1.0 / 512.0 {
        return false;
    }
    if diff > 1.0 / 32.0 {
        // Upright item rendering needs the original FluidHandler capability and tags.
        item.angle += 1;
    }
    item.pos += diff * 0.25;
    true
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    let (Some(&stacks), Some(&displays)) = (STACKS.get(), DISPLAYS.get()) else {
        return;
    };
    let mut depots = std::mem::take(&mut kinetics.depots);
    for (&pos, depot) in &mut depots {
        if let Some(item) = &mut depot.held_item {
            depot.dirty |= slide(item);
        }
        depot.process(world, kinetics, pos);
        if let Some(item) = &mut depot.held_item {
            let face = (item.in_direction as u8).min(5);
            let mut position = center(pos, 15.0 / 16.0);
            if face_axis(face) != 1 {
                let [x, _, z] = FACES[usize::from(face)].map(f64::from);
                let diff = f64::from(0.5 - item.pos);
                position[0] -= x * diff;
                position[2] -= z * diff;
                if face_axis(face) == 2 {
                    position[0] += f64::from(item.offset);
                } else {
                    position[2] -= f64::from(item.offset);
                }
            }
            let display = item_display(position, item.angle, item.places_block(stacks));
            render_stack(world, displays, item, display);
        }
        for (&slot, item) in &mut depot.processing_output_buffer {
            let (sin, cos) =
                (f64::from(slot) * std::f64::consts::TAU / f64::from(OUTPUT_SLOTS)).sin_cos();
            let mut position = center(pos, 15.0 / 16.0);
            position[0] += 0.35 * cos;
            position[2] -= 0.35 * sin;
            let display = item_display(position, item.angle, item.places_block(stacks));
            render_stack(world, displays, item, display);
        }
        if depot.dirty
            && let Ok(bytes) = depot.save()
            && world.set_entity(pos, &bytes)
        {
            depot.dirty = false;
        }
    }
    kinetics.depots = depots;
}

impl DepotBehaviour {
    /// `DepotBehaviour.tick` receives a centered stack, then advances any held processing.
    fn process(&mut self, world: World<'_>, kinetics: &mut Kinetics, pos: Pos) {
        let Some(item) = &mut self.held_item else {
            return;
        };
        if 0.5 - item.pos >= 1.0 / 16.0 {
            return;
        }
        if !item.locked
            && (kinetics
                .get(Pos {
                    y: pos.y + 2,
                    ..pos
                })
                .is_none_or(|be| be.press.is_none())
                || press::is_blocked(world, pos))
        {
            return;
        }
        match press::process_item(kinetics, pos, item) {
            ProcessingResult::Pass => {
                self.dirty |= item.locked;
                item.locked = false;
            }
            ProcessingResult::Hold => {
                self.dirty |= !item.locked;
                item.locked = true;
            }
            ProcessingResult::Convert(outputs) => {
                item.locked = true;
                let rest = count(&item.item) - 1;
                if rest == 0 {
                    remove_display(world, item);
                    self.held_item = None;
                } else {
                    item.resize(world, rest);
                    item.pos = 0.5;
                    item.prev_pos = 0.5;
                }
                for output in outputs {
                    if self.held_item.is_none() {
                        let mut product = TransportedItemStack::new(output);
                        product.pos = 0.5;
                        product.prev_pos = 0.5;
                        product.locked = true;
                        self.held_item = Some(product);
                    } else {
                        self.insert_output(world, pos, output);
                    }
                }
                self.dirty = true;
            }
        }
    }

    fn insert_output(&mut self, world: World<'_>, pos: Pos, output: Value) {
        let mut remain = count(&output);
        let template = TransportedItemStack::new(output);
        let capacity = template.max_count();
        for slot in self.processing_output_buffer.values_mut() {
            if with_count(&slot.item, 1) == with_count(&template.item, 1) {
                let occupied = count(&slot.item);
                let inserted = (capacity - occupied).max(0).min(remain);
                if inserted > 0 {
                    slot.resize(world, occupied + inserted);
                    remain -= inserted;
                }
            }
        }
        for i in 0..OUTPUT_SLOTS {
            if remain <= 0 || capacity <= 0 {
                break;
            }
            if let std::collections::btree_map::Entry::Vacant(slot) =
                self.processing_output_buffer.entry(i)
            {
                let inserted = capacity.min(remain);
                slot.insert(TransportedItemStack::new(with_count(
                    &template.item,
                    inserted,
                )));
                remain -= inserted;
            }
        }
        if remain > 0
            && let (Some(&stacks), Ok(bytes)) = (
                STACKS.get(),
                fastnbt::to_bytes(&with_count(&template.item, remain)),
            )
        {
            stacks.spawn(world, center(pos, 1.0), [0.0; 3], &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ingots(amount: i32) -> TransportedItemStack {
        TransportedItemStack::new(Value::Compound(HashMap::from([
            ("id".into(), Value::String("minecraft:iron_ingot".into())),
            ("count".into(), Value::Int(amount)),
        ])))
    }

    #[test]
    fn direct_input_caps_stack_and_blocks_while_output_is_waiting() {
        let mut depot = DepotBehaviour::default();
        depot.processing_output_buffer.insert(6, ingots(8));
        assert_eq!(depot.insert_from_side(&ingots(79), 3, 64), 0);
        depot.processing_output_buffer.clear();
        assert_eq!(depot.insert_from_side(&ingots(79), 3, 64), 64);
        assert_eq!(depot.insert_from_side(&ingots(2), UP, 64), 0);
        let held = depot.held_item.as_mut().expect("transported item");
        assert_eq!(count(&held.item), 64);
        assert_eq!((held.pos, held.prev_pos, held.in_direction), (0.0, 0.0, 3));
        assert!(slide(held));
        assert_eq!((held.pos, held.prev_pos), (0.125, 0.0));
    }

    #[test]
    fn saves_held_stack_and_sparse_output_slots_in_create_nbt() {
        let mut depot = DepotBehaviour {
            held_item: Some(ingots(2)),
            ..DepotBehaviour::default()
        };
        depot.processing_output_buffer.insert(7, ingots(12));
        let bytes = depot.save().expect("depot NBT");
        let raw: DepotData = fastnbt::from_bytes(&bytes).expect("saved fields");
        assert_eq!(raw.output_buffer.size, 8);
        let [Value::Compound(item)] = raw.output_buffer.items.as_slice() else {
            panic!("expected one saved output");
        };
        assert_eq!(item.get("Slot"), Some(&Value::Int(7)));
        let loaded = DepotBehaviour::decode(&bytes).expect("loaded depot");
        assert_eq!(count(&loaded.held_item.expect("held stack").item), 2);
        assert_eq!(count(&loaded.processing_output_buffer[&7].item), 12);
    }

    #[test]
    fn analog_signal_counts_outputs_and_clamps() {
        let mut depot = DepotBehaviour::default();
        assert_eq!(depot.get_analog_output_signal(), 0);
        depot.processing_output_buffer.insert(0, ingots(32));
        assert_eq!(depot.get_analog_output_signal(), 8);
        depot.processing_output_buffer.insert(1, ingots(64));
        assert_eq!(depot.get_analog_output_signal(), 15);
    }
}
