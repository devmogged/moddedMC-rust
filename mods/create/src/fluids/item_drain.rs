//! Create 6.0.10 ItemDrainBlock and its transported item.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, Content, ContentEvent, ShapeBox, State, World},
    entities::{ENTITY_ALIVE, Entity},
    interaction::{BlockUseResult, UseBlock},
};

use super::{network::FluidHandler, pipe_connection::FluidStack, smart_tank, tank::TankInventory};
use crate::{
    DISPLAYS, FACES, STACKS,
    belt_inventory::{
        self, TransportedItemStack, count, item_display, remove_display, render_stack, with_count,
    },
    depot, face_axis,
    kinetics::{Kind, Kinetics},
    offset,
    processing_recipes::item_id,
};

pub const ID: &str = "create:item_drain";
const CAPACITY: u32 = 1_500;
const FILLING_TIME: i32 = 20;
const EMPTY_TICK: i32 = 5;
const MOVEMENT: f32 = 1.0 / 8.0;
const SURFACE: f64 = 13.0 / 16.0;
// Pumpkin's 26.3 stack serializer writes Potion::WATER as a registry number.
const WATER_POTION_ID: i32 = 0;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Item Drain",
            entity: ID,
            item_model: "create:item/item_drain",
            states: &[State {
                properties: "",
                physical: "minecraft:copper_block",
                model: "create:block/item_drain",
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[ShapeBox {
                    min: [0, 0, 0],
                    max: [16, 13, 16],
                }],
            }],
        },
        None,
    )
}

pub fn load(world: World<'_>, pos: Pos) -> Option<TankInventory> {
    smart_tank::load(&smart_tank::read(world, pos)?, CAPACITY)
}

pub(super) fn emptying(item: &Value) -> Option<(FluidStack, Value)> {
    let (fluid, amount, container) = match item_id(item)? {
        "minecraft:water_bucket" => ("minecraft:water", 1_000, "minecraft:bucket"),
        "minecraft:lava_bucket" => ("minecraft:lava", 1_000, "minecraft:bucket"),
        "minecraft:milk_bucket" => ("minecraft:milk", 1_000, "minecraft:bucket"),
        "minecraft:potion" if is_plain_water(item) => {
            ("minecraft:water", 250, "minecraft:glass_bottle")
        }
        _ => return None,
    };
    Some((
        FluidStack {
            id: fluid.to_owned(),
            amount,
            components: None,
        },
        Value::Compound(HashMap::from([
            ("id".to_owned(), Value::String(container.to_owned())),
            ("count".to_owned(), Value::Int(1)),
        ])),
    ))
}

fn is_plain_water(item: &Value) -> bool {
    let Value::Compound(stack) = item else {
        return false;
    };
    let Some(Value::Compound(components)) = stack.get("components") else {
        return false;
    };
    match components.get("minecraft:potion_contents") {
        Some(Value::String(potion)) => potion == "minecraft:water",
        Some(Value::Compound(potion)) => {
            (matches!(potion.get("potion"), Some(Value::String(id)) if id == "minecraft:water")
                || potion.get("potion") == Some(&Value::Int(WATER_POTION_ID)))
                && potion.get("custom_effects").is_none_or(
                    |effects| matches!(effects, Value::List(effects) if effects.is_empty()),
                )
        }
        _ => false,
    }
}

#[derive(Default)]
pub struct ItemDrain {
    held: Option<TransportedItemStack>,
    processing_ticks: i32,
    dirty: bool,
}

impl ItemDrain {
    fn load(world: World<'_>, pos: Pos) -> Self {
        let Some(data) = smart_tank::read(world, pos) else {
            return Self::default();
        };
        let held = data
            .get("HeldItem")
            .and_then(|value| fastnbt::from_value(value).ok());
        let processing_ticks = match data.get("ProcessingTicks") {
            Some(Value::Int(ticks)) => (*ticks).clamp(0, FILLING_TIME),
            _ => 0,
        };
        Self {
            held,
            processing_ticks,
            dirty: false,
        }
    }

    pub fn insert(&mut self, item: &TransportedItemStack, side: u8) -> i32 {
        if self.held.is_some() {
            return 0;
        }
        let limit = if emptying(&item.item).is_some() {
            1
        } else {
            item.max_count()
        };
        let accepted = count(&item.item).min(limit);
        if accepted <= 0 {
            return 0;
        }
        let mut held = item.copy_with_item(with_count(&item.item, accepted));
        held.in_direction = i32::from(side);
        held.pos = if face_axis(side) == 1 { 0.5 } else { 0.0 };
        held.prev_pos = held.pos;
        held.prev_offset = held.offset;
        self.held = Some(held);
        self.processing_ticks = 0;
        self.dirty = true;
        accepted
    }

    fn persist(&mut self, world: World<'_>, pos: Pos, tank: Option<&TankInventory>) -> bool {
        let Some(mut data) = smart_tank::read(world, pos) else {
            return false;
        };
        if let Some(held) = &self.held {
            let Ok(value) = fastnbt::to_value(held) else {
                return false;
            };
            data.insert("HeldItem".to_owned(), value);
        } else {
            data.remove("HeldItem");
        }
        data.insert(
            "ProcessingTicks".to_owned(),
            Value::Int(self.processing_ticks),
        );
        if tank.is_some_and(|tank| !smart_tank::store(&mut data, tank))
            || !smart_tank::write(world, pos, &data)
        {
            return false;
        }
        self.dirty = false;
        true
    }

    fn advance(&mut self, world: World<'_>, pos: Pos) {
        let next = self.processing_ticks - 1;
        if next >= EMPTY_TICK {
            let Some((fluid, output)) = self.held.as_ref().and_then(|held| emptying(&held.item))
            else {
                self.processing_ticks = 0;
                self.dirty = true;
                return;
            };
            let Some(mut tank) = load(world, pos) else {
                return;
            };
            if tank.fill(&fluid, true) != fluid.amount {
                self.dirty |= self.processing_ticks != FILLING_TIME;
                self.processing_ticks = FILLING_TIME;
                return;
            }
            if next == EMPTY_TICK {
                let Some(previous) = self.held.take() else {
                    return;
                };
                self.held = Some(previous.copy_with_item(output));
                let previous_ticks = self.processing_ticks;
                self.processing_ticks = next;
                tank.fill(&fluid, false);
                if self.persist(world, pos, Some(&tank)) {
                    remove_display(world, &previous);
                } else {
                    self.held = Some(previous);
                    self.processing_ticks = previous_ticks;
                }
                return;
            }
        }
        self.processing_ticks = next;
        self.dirty = true;
    }
}

pub fn on_landed(pos: Pos, drain: &mut ItemDrain, entity: Entity<'_>) {
    let Some(view) = entity.view() else { return };
    if drain.held.is_some()
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
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
    else {
        return;
    };
    let item = TransportedItemStack::new(item);
    let [x, _, z] = view.motion;
    let side = if x.abs() > z.abs() {
        if x > 0.0 { 5 } else { 4 }
    } else if z > 0.0 {
        3
    } else {
        2
    };
    let accepted = drain.insert(&item, side);
    if accepted > 0 {
        let remaining = count(&item.item) - accepted;
        let bytes = if remaining == 0 {
            Some(Vec::new())
        } else {
            fastnbt::to_bytes(&with_count(&item.item, remaining)).ok()
        };
        if !bytes.is_some_and(|bytes| stacks.set_entity_stack(entity, &bytes)) {
            drain.held = None;
        }
    }
}

fn position(pos: Pos, progress: f32, side: u8) -> [f64; 3] {
    let [x, _, z] = FACES[usize::from(side)].map(f64::from);
    let along = f64::from(progress - 0.5);
    [
        f64::from(pos.x) + 0.5 + x * along,
        f64::from(pos.y) + 15.0 / 16.0,
        f64::from(pos.z) + 0.5 + z * along,
    ]
}

fn export(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
    side: u8,
) -> i32 {
    let next = offset(pos, FACES[usize::from(side)]);
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[next], &mut state) || state[0] == content::UNLOADED_STATE {
        return 0;
    }
    let mut forwarded = item.clone();
    forwarded.clear_display();
    if let Some(depot) = kinetics.depots.get_mut(&next) {
        return depot::try_inserting_from_side(world, next, depot, &forwarded, side);
    }
    if let Some(drain) = kinetics.item_drains.get_mut(&next) {
        return drain.insert(&forwarded, side);
    }
    if matches!(kinetics.kind(next), Some(Kind::Belt(_))) {
        return if belt_inventory::try_inserting_from_side(
            kinetics, None, next, &forwarded, side, false,
        ) {
            count(&item.item)
        } else {
            0
        };
    }
    let (Some(stacks), Some(bytes)) = (STACKS.get(), item.bytes()) else {
        return 0;
    };
    let [x, _, z] = FACES[usize::from(side)].map(f64::from);
    if world.face_sturdy(next, side ^ 1)
        || stacks
            .spawn(
                world,
                position(pos, 1.25, side),
                [x / 8.0, 1.0 / 8.0, z / 8.0],
                &bytes,
            )
            .is_none()
    {
        return 0;
    }
    count(&item.item)
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    let (Some(&stacks), Some(&displays)) = (STACKS.get(), DISPLAYS.get()) else {
        return;
    };
    let mut positions: Vec<_> = kinetics.item_drains.keys().copied().collect();
    positions.sort_unstable_by_key(|pos| (pos.x, pos.y, pos.z));
    for pos in positions {
        let Some(mut drain) = kinetics.item_drains.remove(&pos) else {
            continue;
        };
        if drain.processing_ticks > 0 {
            drain.advance(world, pos);
        } else if let Some(item) = &mut drain.held {
            item.prev_pos = item.pos;
            let next = item.pos + MOVEMENT;
            if item.pos < 0.5 && next >= 0.5 && emptying(&item.item).is_some() {
                item.pos = 0.5;
                drain.processing_ticks = FILLING_TIME;
                drain.dirty = true;
            } else {
                item.pos = next.min(1.0);
                drain.dirty |= item.pos != item.prev_pos;
                if next > 1.0 {
                    let side = item.in_direction.clamp(2, 5) as u8;
                    let inserted = export(world, kinetics, pos, item, side);
                    if inserted > 0 {
                        let remaining = count(&item.item) - inserted;
                        if remaining == 0 {
                            remove_display(world, item);
                            drain.held = None;
                        } else {
                            item.resize(world, remaining);
                        }
                        drain.dirty = true;
                    }
                }
            }
        }
        if let Some(item) = &mut drain.held {
            let side = item.in_direction.clamp(2, 5) as u8;
            let display = item_display(
                position(pos, item.pos, side),
                item.angle,
                item.places_block(stacks),
            );
            render_stack(world, displays, item, display);
        }
        if drain.dirty {
            drain.persist(world, pos, None);
        }
        kinetics.item_drains.insert(pos, drain);
    }
}

pub fn event(world: World<'_>, drains: &mut HashMap<Pos, ItemDrain>, event: &ContentEvent) {
    let pos = event.position;
    if super::runtime::is_item_drain(event.state) && event.reason != content::UNLOADED {
        if event.reason != content::NEIGHBOR_CHANGED {
            drains
                .entry(pos)
                .or_insert_with(|| ItemDrain::load(world, pos));
        }
        return;
    }
    if event.reason == content::NEIGHBOR_CHANGED {
        return;
    }
    if let Some(drain) = drains.remove(&pos)
        && let Some(held) = drain.held
    {
        remove_display(world, &held);
        if !matches!(
            event.reason,
            content::UNLOADED | content::MOVED_STATE_CHANGED
        ) && let (Some(stacks), Some(bytes)) = (STACKS.get(), held.bytes())
        {
            stacks.spawn(world, position(pos, 0.5, 3), [0.0, 0.125, 0.0], &bytes);
        }
    }
}

pub fn use_item_on(
    drains: &mut HashMap<Pos, ItemDrain>,
    context: &mut UseBlock<'_>,
) -> BlockUseResult {
    let bytes = context.stack.read();
    let item = bytes
        .as_deref()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(bytes).ok());
    let Some((fluid, output)) = item.as_ref().and_then(emptying) else {
        if let Some(bytes) = bytes
            && STACKS
                .get()
                .and_then(|stacks| stacks.info(&bytes))
                .is_some_and(|info| info.block)
        {
            return BlockUseResult::TryEmptyHand;
        }
        let drain = drains
            .entry(context.position)
            .or_insert_with(|| ItemDrain::load(context.world, context.position));
        if let Some(held) = &drain.held {
            if !held.bytes().is_some_and(|bytes| context.give(&bytes)) {
                return BlockUseResult::Fail;
            }
            remove_display(context.world, held);
            drain.held = None;
            drain.processing_ticks = 0;
            drain.dirty = true;
        }
        return BlockUseResult::Success;
    };
    let (world, pos) = (context.world, context.position);
    let Some(mut tank) = load(world, pos) else {
        return BlockUseResult::Fail;
    };
    if tank.fill(&fluid, true) != fluid.amount {
        return BlockUseResult::Success;
    }
    let Ok(output) = fastnbt::to_bytes(&output) else {
        return BlockUseResult::Fail;
    };
    tank.fill(&fluid, false);
    if !smart_tank::save(world, pos, &tank) {
        return BlockUseResult::Fail;
    }
    if !context.creative {
        let exchanged = if context.stack.count() == 1 {
            context.stack.write(&output)
        } else if context.give(&output) {
            context.stack.shrink(1);
            true
        } else {
            false
        };
        if !exchanged {
            tank.drain(&fluid, false);
            smart_tank::save(world, pos, &tank);
            return BlockUseResult::Fail;
        }
    }
    BlockUseResult::Success
}
