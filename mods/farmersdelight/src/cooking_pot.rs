//! Farmer's Delight 1.3.4 CookingPotBlockEntity cooking lifecycle model.
//! Exact source is CookingPotBlockEntity.cookingTick/processCooking/canCook.
//! This is a tested mod-side algorithm, NOT a registered playable block.
//! Actual ItemStack comparisons, default remainders and world calls must be
//! supplied by the authoritative Minecraft 26.3 host when integrated.

use std::{array, collections::BTreeMap};

use crate::{cooking::CookingPotRecipe, cutting::ResultItem};

pub const INPUT_SLOTS: usize = 6;
pub const MEAL_DISPLAY_SLOT: usize = 6;
pub const CONTAINER_SLOT: usize = 7;
pub const OUTPUT_SLOT: usize = 8;
pub const INVENTORY_SIZE: usize = OUTPUT_SLOT + 1;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CookingTick {
    pub cooked: bool,
    pub inventory_changed: bool,
    pub moved_to_output: u32,
    pub containers_used: u32,
    /// Original ingredient crafting remainders are ejected into the world.
    /// The host must apply these, not silently discard them.
    pub ejected: Vec<ResultItem>,
}

#[derive(Clone, Debug)]
pub struct CookingPotState {
    slots: [Option<ResultItem>; INVENTORY_SIZE],
    pub cook_time: i32,
    pub cook_time_total: i32,
    /// getOutputContainer after the last completed recipe; not necessarily
    /// the original explicit recipe override.
    pub meal_container: Option<ResultItem>,
    pub used_recipes: BTreeMap<String, u32>,
}

impl Default for CookingPotState {
    fn default() -> Self {
        Self {
            slots: array::from_fn(|_| None),
            cook_time: 0,
            cook_time_total: 0,
            meal_container: None,
            used_recipes: BTreeMap::new(),
        }
    }
}

impl CookingPotState {
    pub fn slot(&self, index: usize) -> Option<&ResultItem> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    pub fn set_slot(&mut self, index: usize, stack: Option<ResultItem>) -> bool {
        if index >= INVENTORY_SIZE
            || stack
                .as_ref()
                .is_some_and(|s| s.id.is_empty() || s.count == 0)
        {
            return false;
        }
        self.slots[index] = stack;
        true
    }

    pub fn has_input(&self) -> bool {
        self.slots[..INPUT_SLOTS].iter().any(Option::is_some)
    }

    pub fn input_ids(&self) -> [Option<&str>; INPUT_SLOTS] {
        array::from_fn(|index| self.slots[index].as_ref().map(|stack| stack.id.as_str()))
    }

    /// Original canCook: output matches the meal already stored and there is
    /// enough space, with the special internal meal-slot capacity of at least 64.
    pub fn can_cook(
        &self,
        recipe: &CookingPotRecipe,
        max_stack_count: &impl Fn(&str) -> u32,
        same_components: &impl Fn(&ResultItem, &ResultItem) -> bool,
    ) -> bool {
        if !self.has_input() || recipe.result.id.is_empty() || recipe.result.count == 0 {
            return false;
        }
        let Some(stored) = self.slot(MEAL_DISPLAY_SLOT) else {
            return true;
        };
        if !same_components(stored, &recipe.result) {
            return false;
        }
        let combined = u64::from(stored.count) + u64::from(recipe.result.count);
        combined <= u64::from(max_stack_count(&stored.id).max(64))
            || combined <= u64::from(max_stack_count(&recipe.result.id))
    }

    fn decay(&mut self) {
        self.cook_time = self.cook_time.saturating_sub(2).max(0);
        if self.cook_time_total >= 0 {
            self.cook_time = self.cook_time.min(self.cook_time_total);
        }
    }

    /// Source ingredient-remainder overrides used only if the item lacks a
    /// crafting remaining item from the Minecraft component registry.
    pub fn ingredient_remainder_override(id: &str) -> Option<&'static str> {
        match id {
            "minecraft:powder_snow_bucket"
            | "minecraft:axolotl_bucket"
            | "minecraft:cod_bucket"
            | "minecraft:pufferfish_bucket"
            | "minecraft:salmon_bucket"
            | "minecraft:tropical_fish_bucket" => Some("minecraft:bucket"),
            "minecraft:suspicious_stew"
            | "minecraft:mushroom_stew"
            | "minecraft:rabbit_stew"
            | "minecraft:beetroot_soup" => Some("minecraft:bowl"),
            "minecraft:potion"
            | "minecraft:splash_potion"
            | "minecraft:lingering_potion"
            | "minecraft:experience_bottle" => Some("minecraft:glass_bottle"),
            _ => None,
        }
    }

    fn shrink(&mut self, index: usize, amount: u32) {
        let Some(stack) = self.slots[index].as_mut() else {
            return;
        };
        if stack.count <= amount {
            self.slots[index] = None;
        } else {
            stack.count -= amount;
        }
    }

    /// Exact CookingPotBlockEntity.cookingTick order:
    /// (1) heated input recipe / cook or decay; (2) serve meal to output.
    /// Source recipes are selected externally by the recipe manager.
    ///
    /// The supplied callbacks MUST refer to the host's canonical data:
    /// item maximum counts, crafting remainders and exact item components.
    pub fn tick(
        &mut self,
        heated: bool,
        selected_recipe: Option<(&str, &CookingPotRecipe)>,
        has_tag: impl Fn(&str, &str) -> bool,
        max_stack_count: impl Fn(&str) -> u32,
        crafting_remainder: impl Fn(&ResultItem) -> Option<ResultItem>,
        same_components: impl Fn(&ResultItem, &ResultItem) -> bool,
    ) -> CookingTick {
        let mut event = CookingTick::default();
        if heated && self.has_input() {
            let valid = selected_recipe.filter(|(_, recipe)| {
                recipe.matching_slots(&self.input_ids(), &has_tag).is_some()
                    && self.can_cook(recipe, &max_stack_count, &same_components)
            });
            if let Some((recipe_id, recipe)) = valid {
                self.cook_time = self.cook_time.saturating_add(1);
                self.cook_time_total = recipe.cookingtime;
                if self.cook_time >= self.cook_time_total {
                    self.cook_time = 0;
                    self.meal_container = recipe
                        .container
                        .clone()
                        .or_else(|| crafting_remainder(&recipe.result));
                    match self.slots[MEAL_DISPLAY_SLOT].as_mut() {
                        None => self.slots[MEAL_DISPLAY_SLOT] = Some(recipe.result.clone()),
                        Some(stored) if same_components(stored, &recipe.result) => {
                            stored.count += recipe.result.count;
                        }
                        Some(_) => unreachable!("can_cook checked matching result"),
                    }
                    *self.used_recipes.entry(recipe_id.to_owned()).or_default() += 1;
                    for index in 0..INPUT_SLOTS {
                        if let Some(input) = self.slots[index].as_ref()
                            && let Some(remainder) = crafting_remainder(input).or_else(|| {
                                Self::ingredient_remainder_override(&input.id).map(|id| {
                                    ResultItem {
                                        id: id.to_owned(),
                                        count: 1,
                                        components: None,
                                    }
                                })
                            })
                        {
                            event.ejected.push(remainder);
                        }
                        self.shrink(index, 1);
                    }
                    event.cooked = true;
                    event.inventory_changed = true;
                }
            } else {
                self.decay();
            }
        } else if self.cook_time > 0 {
            self.decay();
        }

        // Always perform meal output after cooking, even on the same tick.
        if self.slot(MEAL_DISPLAY_SLOT).is_some() {
            let meal = self.slot(MEAL_DISPLAY_SLOT).unwrap();
            let needs_container =
                self.meal_container.is_some() || crafting_remainder(meal).is_some();
            if !needs_container {
                event.moved_to_output =
                    self.move_meal_to_output(&max_stack_count, &same_components);
                event.inventory_changed = true; // original does so even when output is blocked
            } else if self.slot(CONTAINER_SLOT).is_some() {
                let (moved, used) =
                    self.use_stored_containers_on_meal(&max_stack_count, &same_components);
                event.moved_to_output = moved;
                event.containers_used = used;
                event.inventory_changed = true;
            }
        }
        event
    }

    fn move_meal_to_output(
        &mut self,
        max_stack_count: &impl Fn(&str) -> u32,
        same_components: &impl Fn(&ResultItem, &ResultItem) -> bool,
    ) -> u32 {
        let Some(meal) = self.slots[MEAL_DISPLAY_SLOT].as_ref() else {
            return 0;
        };
        let output_count = self.slot(OUTPUT_SLOT).map_or(0, |output| output.count);
        let limit = max_stack_count(&meal.id).saturating_sub(output_count);
        let amount = meal.count.min(limit);
        if amount == 0 {
            return 0;
        }
        if self
            .slot(OUTPUT_SLOT)
            .is_some_and(|output| !same_components(output, meal))
        {
            return 0;
        }
        self.transfer_meal_to_output(amount);
        amount
    }

    fn use_stored_containers_on_meal(
        &mut self,
        max_stack_count: &impl Fn(&str) -> u32,
        same_components: &impl Fn(&ResultItem, &ResultItem) -> bool,
    ) -> (u32, u32) {
        let (Some(meal), Some(containers), Some(required)) = (
            self.slot(MEAL_DISPLAY_SLOT),
            self.slot(CONTAINER_SLOT),
            self.meal_container.as_ref(),
        ) else {
            return (0, 0);
        };
        // Original isContainerValid compares the item ID, not components.
        if containers.id != required.id {
            return (0, 0);
        }
        let output_count = self.slot(OUTPUT_SLOT).map_or(0, |stack| stack.count);
        let output_max = self.slot(OUTPUT_SLOT).map_or_else(
            || max_stack_count(&meal.id),
            |stack| max_stack_count(&stack.id),
        );
        if output_count >= output_max {
            return (0, 0);
        }
        let amount = meal
            .count
            .min(containers.count)
            .min(max_stack_count(&meal.id).saturating_sub(output_count));
        if amount == 0 {
            return (0, 0);
        }
        if self
            .slot(OUTPUT_SLOT)
            .is_some_and(|output| !same_components(output, meal))
        {
            return (0, 0);
        }
        self.shrink(CONTAINER_SLOT, amount);
        self.transfer_meal_to_output(amount);
        (amount, amount)
    }

    fn transfer_meal_to_output(&mut self, amount: u32) {
        let mut transfer = self.slots[MEAL_DISPLAY_SLOT].as_ref().unwrap().clone();
        transfer.count = amount;
        match self.slots[OUTPUT_SLOT].as_mut() {
            None => self.slots[OUTPUT_SLOT] = Some(transfer),
            Some(output) => output.count += amount,
        }
        self.shrink(MEAL_DISPLAY_SLOT, amount);
    }

    /// Source useHeldItemOnMeal, with the caller's actual held container stack.
    /// The source requires the explicitly remembered meal container item.
    pub fn use_held_container(&mut self, held: &mut ResultItem) -> Option<ResultItem> {
        let required = self.meal_container.as_ref()?;
        if held.id != required.id || held.count == 0 {
            return None;
        }
        let meal = self.slots[MEAL_DISPLAY_SLOT].as_ref()?.clone();
        held.count -= 1;
        let mut serving = meal;
        serving.count = 1;
        self.shrink(MEAL_DISPLAY_SLOT, 1);
        Some(serving)
    }
}

/// Source HeatableBlockEntity.isHeated; applied by original cooking pots
/// with requiresDirectHeat() == false. State includes only relevant properties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeatBlock {
    pub source_tag: bool,
    pub conductor_tag: bool,
    pub lit: Option<bool>,
}

pub fn is_heated(below: HeatBlock, further_below: HeatBlock, direct_only: bool) -> bool {
    let active = |state: HeatBlock| state.source_tag && state.lit.unwrap_or(true);
    if below.source_tag {
        // If source has lit=false, source returns false without checking deeper.
        return active(below);
    }
    !direct_only && below.conductor_tag && active(further_below)
}

#[cfg(test)]
mod tests;
