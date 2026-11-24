use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, Content, ContentEvent, State, World},
    interaction::{BlockUseResult, PlayerWillDestroy, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{CONTENT, MOBS, STACKS, TAGS};

mod smelting;

pub const ID: &str = "saintsdragons:draconic_crucible";
pub const NAME: &str = "Draconic Crucible";
const GRID_SIZE: usize = 9;
const CAPACITY: [i32; 4] = [0, 5, 11, 17];
const COST: [i32; 4] = [0, 1, 2, 6];
type Stack = HashMap<String, Value>;

static RECIPES: OnceLock<Vec<Recipe>> = OnceLock::new();
static BLOCK_STATE: OnceLock<u32> = OnceLock::new();
static ACTIVE: Mutex<Option<HashMap<usize, HashSet<Pos>>>> = Mutex::new(None);

#[derive(Deserialize)]
struct Ingredient {
    item: String,
}

#[derive(Deserialize)]
struct ResultStack {
    item: String,
    count: i32,
}

#[derive(Deserialize)]
struct Recipe {
    pattern: Option<Vec<String>>,
    #[serde(default)]
    key: HashMap<char, Ingredient>,
    ingredient: Option<Ingredient>,
    result: ResultStack,
    required_heat_level: usize,
    processing_time: i32,
}

impl Recipe {
    fn result_stack(&self) -> Stack {
        stack(&self.result.item, self.result.count)
    }

    fn matches_grid(&self, grid: &[Stack; GRID_SIZE]) -> bool {
        let Some(pattern) = &self.pattern else {
            return false;
        };
        let width = pattern[0].len();
        let height = pattern.len();
        for top in 0..=3 - height {
            for left in 0..=3 - width {
                for mirrored in [false, true] {
                    let matches = grid.iter().enumerate().all(|(slot, stack)| {
                        let row = slot / 3;
                        let col = slot % 3;
                        let expected = if (top..top + height).contains(&row)
                            && (left..left + width).contains(&col)
                        {
                            let x = if mirrored {
                                width - 1 - (col - left)
                            } else {
                                col - left
                            };
                            let code = pattern[row - top].as_bytes()[x] as char;
                            self.key
                                .get(&code)
                                .map(|ingredient| ingredient.item.as_str())
                        } else {
                            None
                        };
                        item_id(stack) == expected
                    });
                    if matches {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct Crucible {
    input: [Stack; GRID_SIZE],
    fuel: Stack,
    output: Stack,
    charge: i32,
    job: Option<Job>,
}

#[derive(Deserialize, Serialize)]
struct Job {
    result: Stack,
    minimum_charge: i32,
    cost: i32,
    ticks: i32,
    elapsed: i32,
}

impl Crucible {
    fn load(world: World<'_>, pos: Pos) -> Self {
        world
            .entity(pos)
            .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, world: World<'_>, pos: Pos) -> bool {
        fastnbt::to_bytes(self).is_ok_and(|bytes| world.set_entity(pos, &bytes))
    }

    fn store_fuel(&mut self, fuel_level: impl Fn(&str) -> Option<usize>) {
        if self.charge == 0
            && let Some(level) = item_id(&self.fuel).and_then(fuel_level)
        {
            self.charge = CAPACITY[level];
            decrement(&mut self.fuel);
        }
    }

    fn start(
        &mut self,
        recipes: &[Recipe],
        max_count: impl Fn(&Stack) -> i32,
        vanilla: impl Fn(&str) -> Option<(Stack, i32)>,
        fuel_level: impl Fn(&str) -> Option<usize>,
    ) -> bool {
        if self.job.is_some() {
            return false;
        }

        // Released shaped recipes have equal priority and are stored in recipe-id order.
        let shaped = recipes
            .iter()
            .find(|recipe| recipe.matches_grid(&self.input));
        let matched = shaped
            .map(|recipe| {
                (
                    recipe.result_stack(),
                    recipe.required_heat_level,
                    recipe.processing_time,
                    None,
                )
            })
            .or_else(|| {
                self.input.iter().enumerate().find_map(|(slot, stack)| {
                    let id = item_id(stack)?;
                    if let Some(recipe) = recipes.iter().find(|recipe| {
                        recipe
                            .ingredient
                            .as_ref()
                            .is_some_and(|input| input.item == id)
                    }) {
                        return Some((
                            recipe.result_stack(),
                            recipe.required_heat_level,
                            recipe.processing_time,
                            Some(slot),
                        ));
                    }
                    vanilla(id).map(|(result, ticks)| (result, 1, ticks, Some(slot)))
                })
            });
        let Some((result, level, ticks, slot)) = matched else {
            return false;
        };
        let total = count(&result) + count(&self.output);
        if (!self.output.is_empty() && !same_item(&self.output, &result))
            || total > max_count(&result)
        {
            return false;
        }

        let minimum_charge = CAPACITY[level - 1];
        let cost = COST[level];
        if self.charge < minimum_charge + cost {
            let Some(fuel) = item_id(&self.fuel).and_then(fuel_level) else {
                return false;
            };
            if CAPACITY[fuel] < minimum_charge + cost {
                return false;
            }
            self.charge = CAPACITY[fuel];
            decrement(&mut self.fuel);
        }
        if let Some(slot) = slot {
            decrement(&mut self.input[slot]);
        } else {
            for stack in &mut self.input {
                decrement(stack);
            }
        }
        self.job = Some(Job {
            result,
            minimum_charge,
            cost,
            ticks,
            elapsed: 0,
        });
        true
    }

    fn advance(
        &mut self,
        recipes: &[Recipe],
        max_count: impl Fn(&Stack) -> i32,
        vanilla: impl Fn(&str) -> Option<(Stack, i32)>,
        fuel_level: impl Fn(&str) -> Option<usize>,
    ) {
        self.store_fuel(&fuel_level);
        let Some(job) = &mut self.job else {
            return;
        };
        let next = job.elapsed + 1;
        let spent = job.cost * next / job.ticks - job.cost * job.elapsed / job.ticks;
        if self.charge <= job.minimum_charge || self.charge - spent < job.minimum_charge {
            return;
        }

        self.charge -= spent;
        job.elapsed = next;
        if next == job.ticks {
            let Some(job) = self.job.take() else {
                return;
            };
            let total = count(&job.result) + count(&self.output);
            self.output = job.result;
            self.output.insert("count".into(), Value::Int(total));
            self.start(recipes, max_count, vanilla, fuel_level);
        }
    }
}

fn stack(id: &str, count: i32) -> Stack {
    HashMap::from([
        ("id".into(), Value::String(id.into())),
        ("count".into(), Value::Int(count)),
    ])
}

fn item_id(stack: &Stack) -> Option<&str> {
    match stack.get("id") {
        Some(Value::String(id)) => Some(id),
        _ => None,
    }
}

fn count(stack: &Stack) -> i32 {
    match stack.get("count") {
        Some(Value::Int(count)) => *count,
        _ => 0,
    }
}

fn decrement(stack: &mut Stack) {
    let count = count(stack);
    if count <= 1 {
        stack.clear();
    } else {
        stack.insert("count".into(), Value::Int(count - 1));
    }
}

fn same_item(left: &Stack, right: &Stack) -> bool {
    item_id(left) == item_id(right) && left.get("components") == right.get("components")
}

fn max_count(stack: &Stack) -> i32 {
    fastnbt::to_bytes(stack)
        .ok()
        .and_then(|bytes| STACKS.get()?.info(&bytes))
        .map_or(0, |info| info.max_count as i32)
}

fn fuel_level(id: &str) -> Option<usize> {
    let tags = TAGS.get()?;
    [
        (3, "saintsdragons:draconic_crucible/fuels/level_3"),
        (2, "saintsdragons:draconic_crucible/fuels/level_2"),
        (1, "saintsdragons:draconic_crucible/fuels/level_1"),
    ]
    .into_iter()
    .find_map(|(level, tag)| tags.contains("item", tag, id).then_some(level))
}

fn state() -> Option<u32> {
    BLOCK_STATE.get().copied().or_else(|| {
        let state = CONTENT.get()?.state(ID)?;
        Some(*BLOCK_STATE.get_or_init(|| state))
    })
}

fn recipes() -> &'static [Recipe] {
    RECIPES.get().map_or(&[], Vec::as_slice)
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let recipes = serde_json::from_str(include_str!("crucible_recipes.json"))
        .map_err(|_| RegistrationError::InvalidDefinition)?;
    let _ = RECIPES.set(recipes);
    let model = if cfg!(feature = "original-assets") {
        "saintsdragons:block/draconic_crucible"
    } else {
        "minecraft:block/blast_furnace"
    };
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: NAME,
            entity: ID,
            item_model: if cfg!(feature = "original-assets") {
                "saintsdragons:item/draconic_crucible"
            } else {
                "minecraft:item/blast_furnace"
            },
            states: &[State {
                properties: "",
                physical: "minecraft:blast_furnace",
                model,
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[],
            }],
        },
        None,
    )
}

pub fn tick(world: World<'_>, events: &[ContentEvent]) {
    let Some(state) = state() else {
        return;
    };
    let mut worlds = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if events.is_empty() && worlds.is_none() {
        return;
    }
    let positions = worlds
        .get_or_insert_with(HashMap::new)
        .entry(world.id())
        .or_default();
    for event in events {
        if event.previous == state && (event.state != state || event.reason == content::UNLOADED) {
            positions.remove(&event.position);
        } else if event.state == state
            && event.reason == content::LOADED
            && Crucible::load(world, event.position).job.is_some()
        {
            positions.insert(event.position);
        }
    }
    positions.retain(|&pos| {
        let mut crucible = Crucible::load(world, pos);
        if crucible.job.is_none() {
            return false;
        }
        crucible.advance(recipes(), max_count, smelting::find, fuel_level);
        crucible.save(world, pos) && crucible.job.is_some()
    });
}

fn grid_slot(hit: [f32; 3]) -> usize {
    let cell = |value: f32| ((value * 3.0) as usize).min(2);
    cell(hit[0]) + 3 * cell(hit[2])
}

pub fn use_block(context: &mut UseBlock<'_>) -> BlockUseResult {
    if state() != Some(context.state) || !context.main_hand {
        return BlockUseResult::Pass;
    }
    // Serialize player inventory transfers with the world tick's block entity writes.
    let mut worlds = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    let mut crucible = Crucible::load(context.world, context.position);
    let top = context.face == 1;
    let changed = if context.with_item {
        let Some(held) = context
            .stack
            .read()
            .and_then(|bytes| fastnbt::from_bytes::<Stack>(&bytes).ok())
        else {
            return BlockUseResult::TryEmptyHand;
        };
        let maximum = max_count(&held);
        let slot = if top {
            &mut crucible.input[grid_slot(context.hit)]
        } else if item_id(&held).and_then(fuel_level).is_some() {
            &mut crucible.fuel
        } else {
            return BlockUseResult::TryEmptyHand;
        };
        if !slot.is_empty() && !same_item(slot, &held) {
            return BlockUseResult::Fail;
        }
        let moved = count(&held).min(maximum - count(slot));
        if moved <= 0 {
            return BlockUseResult::Fail;
        }
        let total = count(slot) + moved;
        *slot = held;
        slot.insert("count".into(), Value::Int(total));
        crucible.store_fuel(fuel_level);
        if !crucible.save(context.world, context.position) {
            return BlockUseResult::Fail;
        }
        if !context.creative {
            context.stack.shrink(moved as u32);
        }
        true
    } else if top {
        let removed = std::mem::take(&mut crucible.input[grid_slot(context.hit)]);
        return take(context, &crucible, removed);
    } else if context.face == 0 {
        let removed = std::mem::take(&mut crucible.fuel);
        return take(context, &crucible, removed);
    } else if !crucible.output.is_empty() {
        let removed = std::mem::take(&mut crucible.output);
        return take(context, &crucible, removed);
    } else {
        let started = crucible.start(recipes(), max_count, smelting::find, fuel_level);
        if started && !crucible.save(context.world, context.position) {
            return BlockUseResult::Fail;
        }
        let status = if let Some(job) = &crucible.job {
            format!(
                "Draconic Crucible: {} ticks remaining, {} charge",
                job.ticks - job.elapsed,
                crucible.charge
            )
        } else {
            format!(
                "Draconic Crucible: {} charge. Use the top for ingredients and the sides for fuel, start and output.",
                crucible.charge
            )
        };
        if let (Some(mobs), Some(player)) = (MOBS.get(), context.player) {
            mobs.message(player, &status, true);
        }
        started
    };
    if changed && crucible.job.is_some() {
        worlds
            .get_or_insert_with(HashMap::new)
            .entry(context.world.id())
            .or_default()
            .insert(context.position);
    }
    BlockUseResult::Success
}

fn take(context: &UseBlock<'_>, crucible: &Crucible, removed: Stack) -> BlockUseResult {
    if removed.is_empty() {
        return BlockUseResult::Success;
    }
    let Ok(bytes) = fastnbt::to_bytes(&removed) else {
        return BlockUseResult::Fail;
    };
    if !crucible.save(context.world, context.position) {
        return BlockUseResult::Fail;
    }
    if !context.give(&bytes)
        && let Ok(stack) = fastnbt::from_bytes::<Stack>(&bytes)
    {
        let mut restored = Crucible::load(context.world, context.position);
        match context.face {
            0 => restored.fuel = stack,
            1 => restored.input[grid_slot(context.hit)] = stack,
            _ => restored.output = stack,
        }
        restored.save(context.world, context.position);
        return BlockUseResult::Fail;
    }
    BlockUseResult::Success
}

pub fn break_block(context: &PlayerWillDestroy<'_>) {
    if state() != Some(context.state) {
        return;
    }
    let Some(stacks) = STACKS.get() else {
        return;
    };
    let _worlds = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    let pos = context.position;
    let at = [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    ];
    let crucible = Crucible::load(context.world, pos);
    for stack in crucible
        .input
        .into_iter()
        .chain([crucible.fuel, crucible.output])
    {
        if !stack.is_empty()
            && let Ok(bytes) = fastnbt::to_bytes(&stack)
        {
            stacks.spawn(context.world, at, [0.0; 3], &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn released_recipes() -> Vec<Recipe> {
        serde_json::from_str(include_str!("crucible_recipes.json")).unwrap()
    }

    #[test]
    fn released_shaped_recipes_match_mirrored_and_translated() {
        let recipes = released_recipes();
        assert_eq!(recipes.len(), 14);
        for recipe in &recipes {
            let Some(pattern) = &recipe.pattern else {
                continue;
            };
            let top = 3 - pattern.len();
            let left = 3 - pattern[0].len();
            let mut grid: [Stack; GRID_SIZE] = Default::default();
            for (row, line) in pattern.iter().enumerate() {
                for (col, code) in line.chars().rev().enumerate() {
                    if let Some(ingredient) = recipe.key.get(&code) {
                        grid[left + col + (top + row) * 3] = stack(&ingredient.item, 1);
                    }
                }
            }
            assert!(recipe.matches_grid(&grid), "{}", recipe.result.item);
            let empty = grid.iter_mut().find(|stack| stack.is_empty());
            if let Some(empty) = empty {
                *empty = stack("minecraft:dirt", 1);
                assert!(!recipe.matches_grid(&grid), "{}", recipe.result.item);
            }
        }
    }

    #[test]
    fn crucible_uses_the_released_charge_threshold_and_reserves_the_result() {
        let recipes = released_recipes();
        let mut machine = Crucible {
            charge: 11,
            ..Default::default()
        };
        for (slot, item) in [
            (1, "saintsdragons:dragonheart_chunk"),
            (2, "saintsdragons:dragonheart_chunk"),
            (4, "saintsdragons:dragonheart_chunk"),
            (5, "saintsdragons:dragonheart_chunk"),
            (7, "saintsdragons:worldroot_ingot"),
            (8, "saintsdragons:worldroot_ingot"),
        ] {
            machine.input[slot] = stack(item, 2);
        }
        assert!(!machine.start(
            &recipes,
            |_| 64,
            |_| None,
            |id| (id == "minecraft:dragon_breath").then_some(3)
        ));
        assert_eq!(count(&machine.input[1]), 2);
        machine.fuel = stack("minecraft:dragon_breath", 1);
        assert!(machine.start(
            &recipes,
            |_| 64,
            |_| None,
            |id| (id == "minecraft:dragon_breath").then_some(3)
        ));
        assert_eq!(machine.charge, 17);
        assert!(machine.fuel.is_empty());
        assert_eq!(count(&machine.input[1]), 1);
        for _ in 0..1799 {
            machine.advance(
                &recipes,
                |_| 64,
                |_| None,
                |id| (id == "minecraft:dragon_breath").then_some(3),
            );
        }
        assert_eq!(machine.charge, 12);
        assert!(machine.output.is_empty());
        let bytes = fastnbt::to_bytes(&machine).unwrap();
        let mut machine: Crucible = fastnbt::from_bytes(&bytes).unwrap();
        machine.advance(
            &recipes,
            |_| 64,
            |_| None,
            |id| (id == "minecraft:dragon_breath").then_some(3),
        );
        assert_eq!(machine.charge, 11);
        assert_eq!(
            item_id(&machine.output),
            Some("saintsdragons:dragonheart_alloy")
        );
        assert_eq!(count(&machine.output), 2);
        assert!(machine.job.is_none());
        assert_eq!(count(&machine.input[1]), 1);
    }
}
