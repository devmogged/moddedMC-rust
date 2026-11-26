//! The upward Mechanical Saw's processing inventory.

use fastnbt::Value;
use moddedmc_api::{
    Pos,
    content::World,
    entities::{ENTITY_ALIVE, ENTITY_ITEM, Entity},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    STACKS,
    belt_inventory::{self, TransportedItemStack, count, with_count},
    belt_tunnel::shift,
    filtering::{self, Filter},
    kinetics::{FACES, Kind, Kinetics},
    processing_recipes::{self, Process, RecipeBook},
    saw::SawBlock,
};

const UP: u8 = 1;
const OUTPUT_TICKS: f32 = 20.0;
const NO_RECIPE_TICKS: f32 = 10.0;
const INVENTORY_SLOTS: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub(crate) struct SawData {
    items: Vec<Value>,
    remaining: Option<f32>,
    duration: f32,
    applied: bool,
    recipe_index: usize,
    filter: Filter,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Processing {
    pub(crate) data: SawData,
    displays: Vec<Option<TransportedItemStack>>,
    filter_display: Option<TransportedItemStack>,
}

impl Processing {
    pub(crate) fn load(data: SawData) -> Self {
        Self {
            data,
            ..Self::default()
        }
    }

    pub(crate) fn clear_displays(&mut self, world: World<'_>) {
        for item in self
            .displays
            .drain(..)
            .flatten()
            .chain(self.filter_display.take())
        {
            belt_inventory::remove_display(world, &item);
        }
    }

    pub(crate) fn drop_contents(&self, world: World<'_>, pos: Pos) {
        self.data.filter.drop_item(world, pos);
        let Some(&stacks) = STACKS.get() else {
            return;
        };
        let center = [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5);
        for item in &self.data.items {
            if let Ok(bytes) = fastnbt::to_bytes(item) {
                stacks.spawn(world, center, [0.0; 3], &bytes);
            }
        }
    }

    fn reset_cycle(&mut self) {
        self.data.remaining = None;
        self.data.duration = 0.0;
        self.data.applied = false;
    }

    fn step(&mut self, speed: f32, recipes: &RecipeBook) {
        if self.data.items.is_empty() || speed == 0.0 {
            return;
        }
        if let Some(time) = self.data.remaining {
            let remaining = (time - (speed.abs() / 24.0).clamp(1.0, 128.0)).max(0.0);
            self.data.remaining = Some(remaining);
            if !self.data.applied && remaining < 5.0 {
                let recipes =
                    recipes.matches(Process::Cutting, &self.data.items[0], &self.data.filter);
                if !recipes.is_empty() {
                    if self.data.recipe_index >= recipes.len() {
                        self.data.recipe_index = 0;
                    }
                    let mut items =
                        recipes[self.data.recipe_index].roll_results(count(&self.data.items[0]));
                    items.truncate(INVENTORY_SLOTS - 1);
                    self.data.items = items;
                }
                self.data.applied = true;
                self.data.duration = OUTPUT_TICKS;
                self.data.remaining = Some(OUTPUT_TICKS);
            }
        } else {
            let recipes = recipes.matches(Process::Cutting, &self.data.items[0], &self.data.filter);
            let duration = if recipes.is_empty() {
                NO_RECIPE_TICKS
            } else {
                self.data.recipe_index = (self.data.recipe_index + 1) % recipes.len();
                let recipe = recipes[self.data.recipe_index];
                (recipe.duration.max(1) * (count(&self.data.items[0]) / 5).max(1)) as f32
            };
            self.data.duration = duration;
            self.data.remaining = Some(duration);
        }
    }

    fn output(&mut self, world: World<'_>, kinetics: &mut Kinetics, pos: Pos, direction: u8) {
        let target = shift(pos, direction);
        let direct = kinetics.depots.contains_key(&target)
            || matches!(kinetics.kind(target), Some(Kind::Belt(_)))
            || matches!(kinetics.kind(target), Some(Kind::Saw(block)) if block.facing == UP);
        if direct {
            self.data.items.retain_mut(|item| {
                let amount = count(item);
                let stack = TransportedItemStack::new(item.clone());
                let inserted =
                    crate::funnel::insert(world, kinetics, target, direction ^ 1, &stack, false);
                if inserted > 0 {
                    *item = with_count(item, amount - inserted);
                }
                inserted < amount
            });
        } else if let Some(&stacks) = STACKS.get() {
            let [dx, _, dz] = FACES[usize::from(direction)].map(f64::from);
            let center = [
                f64::from(pos.x) + 0.5 + 0.5 * dx,
                f64::from(pos.y) + 1.0,
                f64::from(pos.z) + 0.5 + 0.5 * dz,
            ];
            for item in self.data.items.drain(..) {
                if let Ok(bytes) = fastnbt::to_bytes(&item) {
                    stacks.spawn(world, center, [dx * 0.0625, 0.125, dz * 0.0625], &bytes);
                }
            }
        }
        if self.data.items.is_empty() {
            self.reset_cycle();
        }
    }

    fn draw(&mut self, world: World<'_>, pos: Pos, block: SawBlock, direction: u8, speed: f32) {
        let [x, z] = block.filter_center();
        let center = [
            f64::from(pos.x) + x,
            f64::from(pos.y) + 0.79,
            f64::from(pos.z) + z,
        ];
        let mut shown = belt_inventory::item_display(center, 0, false);
        shown.scale = 0.19;
        filtering::render(
            world,
            self.data.filter.stack.as_ref(),
            &mut self.filter_display,
            shown,
        );

        while self.displays.len() > self.data.items.len() {
            if let Some(Some(shown)) = self.displays.pop() {
                belt_inventory::remove_display(world, &shown);
            }
        }
        self.displays.resize_with(self.data.items.len(), || None);
        let fraction = if speed != 0.0 && self.data.duration > 0.0 {
            let fraction =
                (self.data.remaining.unwrap_or_default() / self.data.duration).clamp(0.125, 1.0);
            if self.data.applied {
                fraction * 0.5
            } else {
                (1.0 + fraction) * 0.5
            }
        } else {
            0.5
        };
        let [dx, _, dz] = FACES[usize::from(direction)].map(f64::from);
        let offset = f64::from(0.5 - fraction);
        let count = self.data.items.len();
        for (i, (item, visible)) in self.data.items.iter().zip(&mut self.displays).enumerate() {
            let lane = if count > 1 {
                (i as f64 / (count - 1) as f64 - 0.5) * 0.5
            } else {
                0.0
            };
            let point = [
                f64::from(pos.x) + 0.5 + offset * dx + lane * dz,
                f64::from(pos.y) + 13.0 / 16.0,
                f64::from(pos.z) + 0.5 + offset * dz + lane * dx,
            ];
            filtering::render(
                world,
                Some(item),
                visible,
                belt_inventory::item_display(point, 0, false),
            );
        }
    }
}

pub(crate) fn tick(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    block: SawBlock,
    speed: f32,
) {
    if block.facing != UP {
        return;
    }
    let Some(mut processing) = kinetics
        .get_mut(pos)
        .and_then(|be| be.saw_processing.take())
    else {
        return;
    };
    let direction = block.output_direction(speed);
    let active = speed != 0.0 && !processing.data.items.is_empty();
    if active {
        if let Some(recipes) = processing_recipes::current() {
            processing.step(speed, &recipes);
        }
        if processing.data.applied && processing.data.remaining == Some(0.0) {
            processing.output(world, kinetics, pos, direction);
        }
    }
    processing.draw(world, pos, block, direction, speed);
    if let Some(be) = kinetics.get_mut(pos) {
        be.dirty |= active;
        be.saw_processing = Some(processing);
    }
}

pub(crate) fn insert(
    kinetics: &mut Kinetics,
    pos: Pos,
    face: u8,
    item: &Value,
    simulate: bool,
) -> Option<i32> {
    let be = kinetics.get_mut(pos)?;
    let Kind::Saw(block) = be.kind else {
        return None;
    };
    if block.facing != UP
        || face == 0
        || count(item) <= 0
        || be
            .saw_processing
            .as_ref()
            .is_some_and(|processing| !processing.data.items.is_empty())
    {
        return Some(0);
    }
    if !simulate {
        let saw = be.saw_processing.get_or_insert_with(Default::default);
        saw.reset_cycle();
        saw.data.items.push(with_count(item, 1));
        be.dirty = true;
    }
    Some(1)
}

pub(crate) fn entity_inside(world: World<'_>, pos: Pos, block: SawBlock, entity: Entity<'_>) {
    let Some(view) = entity.view() else { return };
    if view.kind & ENTITY_ITEM == 0 {
        crate::drill::entity_inside(world, pos, entity, crate::saw::ID);
        return;
    }
    if block.facing != UP
        || entity.flags() & ENTITY_ALIVE == 0
        || !view.on_ground
        || view.motion[1] > 0.0
        || (view.position[1] - f64::from(pos.y) - 0.75).abs() > 1e-5
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
    crate::with_world(world, |kinetics| {
        if !kinetics.get(pos).is_some_and(|be| be.get_speed() != 0.0)
            || insert(kinetics, pos, UP, &item, true) != Some(1)
        {
            return;
        }
        let remainder = if count(&item) <= 1 {
            Vec::new()
        } else if let Ok(bytes) = fastnbt::to_bytes(&with_count(&item, count(&item) - 1)) {
            bytes
        } else {
            return;
        };
        if stacks.set_entity_stack(entity, &remainder) {
            insert(kinetics, pos, UP, &item, false);
        }
    });
}

pub(crate) fn use_block(
    kinetics: &mut Kinetics,
    context: &mut UseBlock<'_>,
    block: SawBlock,
) -> BlockUseResult {
    if block.facing != UP
        || context.face != UP
        || !context.main_hand
        || !context.player.is_some_and(|player| player.may_build())
    {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(be) = kinetics.get_mut(context.position) else {
        return BlockUseResult::TryEmptyHand;
    };
    let processing = be.saw_processing.get_or_insert_with(Default::default);
    let sample = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok());
    let [x, z] = block.filter_center().map(|coord| coord as f32);
    if (context.hit[0] - x).abs() < 0.125
        && (context.hit[2] - z).abs() < 0.125
        && (sample.is_some() || context.sneaking)
        && processing
            .data
            .filter
            .configure(context, sample.as_ref(), 0.0, false)
    {
        be.dirty = true;
        return BlockUseResult::Success;
    }
    if sample.is_some() || processing.data.items.is_empty() {
        return BlockUseResult::TryEmptyHand;
    }
    processing
        .data
        .items
        .retain(|item| !fastnbt::to_bytes(item).is_ok_and(|bytes| context.give(&bytes)));
    if processing.data.items.is_empty() {
        processing.reset_cycle();
    }
    be.dirty = true;
    BlockUseResult::Success
}
