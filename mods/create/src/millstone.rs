//! Create 6.0.10's MillstoneBlock, MillstoneBlockEntity and MillstoneInventoryHandler.

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, ShapeBox, State, World},
    entities::{ENTITY_ALIVE, ENTITY_ITEM},
    interaction::{BlockUseResult, UseBlock},
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    ENTITIES, ITEMS, STACKS, STONE, StateIds,
    belt_inventory::{count, with_count},
    kinetics::{Kind, Kinetics},
    processing_recipes::{Process, RecipeBook},
};

pub const ID: &str = "create:millstone";
const OUTPUT_SLOTS: usize = 9;
const NO_RECIPE_RETRY: i32 = 100;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: "create:wheat_flour",
                display_name: "Wheat Flour",
                carrier: "minecraft:paper",
                model: "create:item/wheat_flour",
            },
        )?;
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Millstone",
            entity: ID,
            item_model: "create:block/millstone/item",
            states: &[State {
                properties: "",
                physical: STONE,
                model: "create:block/millstone/block",
                rotating_model: "create:block/millstone/inner",
                x: 0,
                y: 0,
                orientation: crate::relay_orientation(1),
                shape: &[
                    ShapeBox {
                        min: [0, 0, 0],
                        max: [16, 6, 16],
                    },
                    ShapeBox {
                        min: [2, 6, 2],
                        max: [14, 16, 14],
                    },
                ],
            }],
        },
        None,
    )
}

pub fn add_states(content: Content, states: &mut StateIds) {
    states.add(content, ID, "", Some(Kind::Millstone));
}

#[derive(Clone, Debug, Default)]
pub struct Millstone {
    timer: i32,
    input: Option<Value>,
    output: [Option<Value>; OUTPUT_SLOTS],
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MillstoneData {
    timer: i32,
    input_inventory: InventoryData,
    output_inventory: InventoryData,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InventoryData {
    size: i32,
    items: Vec<Value>,
}

impl InventoryData {
    pub fn load<const N: usize>(self) -> [Option<Value>; N] {
        let mut slots = std::array::from_fn(|_| None);
        for mut item in self.items {
            let Value::Compound(values) = &mut item else {
                continue;
            };
            let Some(Value::Int(index)) = values.remove("Slot") else {
                continue;
            };
            if let Some(slot) = usize::try_from(index)
                .ok()
                .and_then(|index| slots.get_mut(index))
                && count(&item) > 0
            {
                *slot = Some(item);
            }
        }
        slots
    }

    pub fn save(slots: &[Option<Value>]) -> Self {
        let items = slots
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| {
                let Value::Compound(mut values) = item.clone()? else {
                    return None;
                };
                values.insert("Slot".into(), Value::Int(slot as i32));
                Some(Value::Compound(values))
            })
            .collect();
        Self {
            size: slots.len() as i32,
            items,
        }
    }
}

impl Millstone {
    pub fn load(data: MillstoneData) -> Self {
        let [input] = data.input_inventory.load();
        Self {
            timer: data.timer,
            input,
            output: data.output_inventory.load(),
        }
    }

    pub fn save(&self) -> MillstoneData {
        MillstoneData {
            timer: self.timer,
            input_inventory: InventoryData::save(std::slice::from_ref(&self.input)),
            output_inventory: InventoryData::save(&self.output),
        }
    }

    fn insert(&mut self, item: &Value, recipes: &RecipeBook, simulate: bool) -> i32 {
        if self
            .input
            .as_ref()
            .is_some_and(|held| with_count(held, 1) != with_count(item, 1))
            || recipes.find(Process::Milling, item).is_none()
        {
            return 0;
        }
        let held = self.input.as_ref().map_or(0, count);
        let accepted = count(item).min((max_count(item) - held).max(0));
        if !simulate && accepted > 0 {
            self.input = Some(with_count(item, held + accepted));
        }
        accepted
    }

    fn extract(&mut self, limit: i32, simulate: bool) -> Option<Value> {
        let slot = self.output.iter_mut().find(|slot| slot.is_some())?;
        let item = slot.as_ref()?;
        let taken = limit.min(count(item));
        let result = with_count(item, taken);
        if !simulate {
            let remaining = count(item) - taken;
            *slot = (remaining > 0).then(|| with_count(item, remaining));
        }
        Some(result)
    }

    fn insert_output(&mut self, product: Value, capacity: i32) {
        let mut remaining = count(&product);
        for slot in self.output.iter_mut().flatten() {
            if with_count(slot, 1) == with_count(&product, 1) {
                let held = count(slot);
                let moved = remaining.min((capacity - held).max(0));
                remaining -= moved;
                *slot = with_count(slot, held + moved);
            }
        }
        for slot in self.output.iter_mut().filter(|slot| slot.is_none()) {
            if remaining <= 0 || capacity <= 0 {
                break;
            }
            let moved = capacity.min(remaining);
            *slot = Some(with_count(&product, moved));
            remaining -= moved;
        }
    }

    fn tick(&mut self, speed: f32, recipes: &RecipeBook) -> bool {
        if speed == 0.0 || self.output.iter().flatten().any(|item| count(item) == 64) {
            return false;
        }
        if self.timer > 0 {
            self.timer -= get_processing_speed(speed);
            if self.timer <= 0
                && let Some(item) = &self.input
                && let Some(recipe) = recipes.find(Process::Milling, item)
            {
                let remaining = count(item) - 1;
                self.input = (remaining > 0).then(|| with_count(item, remaining));
                for result in recipe.roll_results(1) {
                    let max = max_count(&result);
                    self.insert_output(result, max);
                }
            }
            return true;
        }
        let Some(item) = &self.input else {
            return false;
        };
        self.timer = recipes
            .find(Process::Milling, item)
            .map_or(NO_RECIPE_RETRY, |recipe| recipe.duration);
        true
    }

    pub fn drop_contents(self, world: World<'_>, pos: Pos) {
        let Some(stacks) = STACKS.get() else { return };
        for item in self
            .input
            .into_iter()
            .chain(self.output.into_iter().flatten())
        {
            if let Ok(bytes) = fastnbt::to_bytes(&item) {
                stacks.spawn(world, center(pos), [0.0, 0.1, 0.0], &bytes);
            }
        }
    }
}

pub fn max_count(item: &Value) -> i32 {
    fastnbt::to_bytes(item)
        .ok()
        .and_then(|bytes| STACKS.get()?.info(&bytes))
        .map_or(0, |info| info.max_count as i32)
}

fn get_processing_speed(speed: f32) -> i32 {
    ((speed / 16.0).abs() as i32).clamp(1, 512)
}

fn center(pos: Pos) -> [f64; 3] {
    [pos.x, pos.y, pos.z].map(|n| f64::from(n) + 0.5)
}

pub fn insert(kinetics: &mut Kinetics, pos: Pos, item: &Value, simulate: bool) -> i32 {
    let Some(recipe_book) = crate::processing_recipes::current() else {
        return 0;
    };
    let Some(be) = kinetics.get_mut(pos) else {
        return 0;
    };
    let Some(millstone) = &mut be.millstone else {
        return 0;
    };
    let accepted = millstone.insert(item, &recipe_book, simulate);
    be.dirty |= !simulate && accepted > 0;
    accepted
}

pub fn extract(kinetics: &mut Kinetics, pos: Pos, limit: i32, simulate: bool) -> Option<Value> {
    let be = kinetics.get_mut(pos)?;
    let item = be.millstone.as_mut()?.extract(limit, simulate)?;
    be.dirty |= !simulate;
    Some(item)
}

pub fn use_item_on(kinetics: &mut Kinetics, context: &mut UseBlock<'_>) -> BlockUseResult {
    if context.stack.read().is_some() {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(be) = kinetics.get_mut(context.position) else {
        return BlockUseResult::TryEmptyHand;
    };
    let Some(millstone) = &mut be.millstone else {
        return BlockUseResult::TryEmptyHand;
    };
    if millstone.output.iter().all(Option::is_none) {
        be.dirty |= give(context, &mut millstone.input);
    } else {
        for slot in &mut millstone.output {
            be.dirty |= give(context, slot);
        }
    }
    BlockUseResult::Success
}

fn give(context: &mut UseBlock<'_>, slot: &mut Option<Value>) -> bool {
    let Some(item) = slot else { return false };
    if fastnbt::to_bytes(item).is_ok_and(|bytes| context.give(&bytes)) {
        *slot = None;
        true
    } else {
        false
    }
}

/// MillstoneBlock.updateEntityAfterFallOn, using the grounded ItemEntity state.
fn take_landed_items(
    world: World<'_>,
    pos: Pos,
    millstone: &mut Millstone,
    recipes: &RecipeBook,
) -> bool {
    let (Some(&entities), Some(&stacks)) = (ENTITIES.get(), STACKS.get()) else {
        return false;
    };
    let [x, y, z] = center(pos);
    let min = [x - 0.5, y - 0.125, z - 0.5];
    let max = [x + 0.5, y + 0.55, z + 0.5];
    let mut changed = false;
    for id in entities.entities_in(world, min, max, 0) {
        changed |= entities
            .with_entity(world, id, |entity| {
                let Some(view) = entity.view() else {
                    return false;
                };
                if view.kind & ENTITY_ITEM == 0
                    || entity.flags() & ENTITY_ALIVE == 0
                    || !view.on_ground
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
                let accepted = millstone.insert(&item, recipes, true);
                if accepted <= 0 {
                    return false;
                }
                let rest = count(&item) - accepted;
                let remainder = if rest == 0 {
                    Vec::new()
                } else {
                    let Ok(bytes) = fastnbt::to_bytes(&with_count(&item, rest)) else {
                        return false;
                    };
                    bytes
                };
                if !stacks.set_entity_stack(entity, &remainder) {
                    return false;
                }
                millstone.insert(&item, recipes, false);
                true
            })
            .unwrap_or(false);
    }
    changed
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    let positions = kinetics.positions(|kind| kind == Kind::Millstone);
    if positions.is_empty() {
        return;
    }
    let Some(recipes) = crate::processing_recipes::current() else {
        return;
    };
    for pos in positions {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let speed = be.get_speed();
        let Some(millstone) = &mut be.millstone else {
            continue;
        };
        be.dirty |= millstone.tick(speed, &recipes);
        be.dirty |= take_landed_items(world, pos, millstone, &recipes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn millstone_saves_slots_and_merges_before_using_empty_space() {
        let item = Value::Compound(HashMap::from([
            ("id".into(), Value::String("minecraft:clay_ball".into())),
            ("count".into(), Value::Int(62)),
        ]));
        let mut millstone = Millstone {
            timer: 5,
            ..Millstone::default()
        };
        millstone.output[7] = Some(item.clone());
        millstone.insert_output(with_count(&item, 4), 64);
        let loaded = Millstone::load(
            fastnbt::from_bytes(&fastnbt::to_bytes(&millstone.save()).expect("saved millstone"))
                .expect("decoded millstone"),
        );
        assert_eq!(loaded.timer, 5);
        assert_eq!(loaded.output[7], Some(with_count(&item, 64)));
        assert_eq!(loaded.output[0], Some(with_count(&item, 2)));
        assert_eq!(get_processing_speed(-15.0), 1);
        assert_eq!(get_processing_speed(255.0), 15);
    }
}
