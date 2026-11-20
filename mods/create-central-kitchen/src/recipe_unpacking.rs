//! RecipeUnpacking.java, Create Central Kitchen 2.6.2b.
//! The source plans a complete package without changing package or inventory.
//! A caller must resolve recipe Ingredient predicates and saved ItemStack components.
//! The type below is a synchronous model; it is not an installed packager hook.

/// A saved item identity and count. `components` must use the caller's
/// canonical component representation, so equal items can be aggregated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    pub item: String,
    pub components: Vec<u8>,
    pub count: i32,
    pub max_stack: i32,
}

impl Stack {
    pub fn new(item: &str, count: i32) -> Self {
        Self {
            item: item.to_owned(),
            components: Vec::new(),
            count,
            max_stack: 64,
        }
    }

    pub fn empty() -> Self {
        Self::new("minecraft:air", 0)
    }

    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item == "minecraft:air"
    }

    fn one(&self) -> Self {
        let mut copy = self.clone();
        copy.count = 1;
        copy
    }

    fn with_count(&self, count: i32) -> Self {
        let mut copy = self.clone();
        copy.count = count;
        copy
    }

    fn is_same_item_same_components(&self, other: &Self) -> bool {
        self.item == other.item && self.components == other.components
    }

    fn matches(&self, other: &Self) -> bool {
        self.count == other.count
            && (self.is_empty() && other.is_empty() || self.is_same_item_same_components(other))
    }
}

/// The caller must translate NeoForge Ingredient::test (including tag and
/// component rules) into the accepted source item identifiers.
#[derive(Debug, Clone)]
pub struct Ingredient {
    pub accepted: Vec<String>,
}

impl Ingredient {
    fn test(&self, stack: &Stack) -> bool {
        self.accepted.iter().any(|id| id == &stack.item)
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub ingredients: Vec<Ingredient>,
}

impl Candidate {
    pub fn new(id: &str, ingredients: Vec<Ingredient>) -> Self {
        // Candidate(Recipe) excludes empty ingredient placeholders.
        Self {
            id: id.to_owned(),
            ingredients: ingredients
                .into_iter()
                .filter(|ingredient| !ingredient.accepted.is_empty())
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub stack: Stack,
    pub count: i32,
}

/// Equivalent to the used IItemHandler methods. A host implementation must
/// preserve NeoForge's simulation and actual insertion semantics.
pub trait ItemHandler {
    fn slots(&self) -> usize;
    fn stack_in_slot(&self, slot: usize) -> Stack;
    fn slot_limit(&self, slot: usize) -> i32;
    fn is_item_valid(&self, slot: usize, stack: &Stack) -> bool;
    /// Return the number of incoming items *not* inserted.
    fn insert_item(&mut self, slot: usize, stack: &Stack, simulate: bool) -> i32;
}

pub struct Device<'a> {
    pub inventory: &'a dyn ItemHandler,
    pub slot_start: i32,
    pub slot_count: i32,
    pub recipes: &'a [Candidate],
}

/// Owns the initial inventory snapshot and each planned slot addition.
#[derive(Debug, Clone)]
pub struct Plan {
    start: usize,
    before: Vec<Stack>,
    additions: Vec<Stack>,
}

impl Plan {
    pub fn additions(&self) -> &[Stack] {
        &self.additions
    }

    /// `RecipeUnpacking.Plan.apply` validates every target and simulated
    /// insertion before changing any inventory slot. The caller must guard
    /// the inventory against concurrent mutations during this synchronous call.
    pub fn apply(&self, inventory: &mut dyn ItemHandler, simulate: bool) -> bool {
        for (index, (before, addition)) in self.before.iter().zip(&self.additions).enumerate() {
            let slot = self.start + index;
            let existing = inventory.stack_in_slot(slot);
            if !existing.matches(before) {
                return false;
            }
            if addition.is_empty() {
                continue;
            }
            let limit = inventory.slot_limit(slot).min(addition.max_stack);
            if (!existing.is_empty() && !existing.is_same_item_same_components(addition))
                || i64::from(existing.count) + i64::from(addition.count) > i64::from(limit)
                || !inventory.is_item_valid(slot, addition)
                || inventory.insert_item(slot, addition, true) != 0
            {
                return false;
            }
        }
        if !simulate {
            for (index, addition) in self.additions.iter().enumerate() {
                if !addition.is_empty() {
                    let _ = inventory.insert_item(self.start + index, addition, false);
                }
            }
        }
        true
    }
}

/// `RecipeUnpacking.plan`: respect context first, existing slots second,
/// then one-item-per-slot and finally uniform bulk batches. A rejected first
/// match does not cause a fallback recipe or a second layout to be tried.
pub fn plan(
    device: &Device<'_>,
    items: &[Stack],
    context: Option<&[ContextEntry]>,
    accepts_recipe: impl Fn(&Candidate) -> bool,
) -> Option<Plan> {
    if !(1..=9).contains(&device.slot_count) || device.slot_start < 0 {
        return None;
    }
    let start = usize::try_from(device.slot_start).ok()?;
    let count = usize::try_from(device.slot_count).ok()?;
    if start.checked_add(count)? > device.inventory.slots() {
        return None;
    }
    let contents = aggregate(items);
    let total: i64 = contents.iter().map(|s| i64::from(s.count)).sum();
    if total == 0 || total > i64::from(i32::MAX) || contents.len() > count {
        return None;
    }

    let mut before = Vec::with_capacity(count);
    let mut occupied = Vec::new();
    let mut existing = Vec::new();
    let mut stored_servings = 0;
    for slot in 0..count {
        let stack = device.inventory.stack_in_slot(start + slot);
        if !stack.is_empty() {
            if stored_servings != 0 && stored_servings != stack.count {
                return None;
            }
            stored_servings = stack.count;
            occupied.push(slot);
            existing.push(stack.one());
        }
        before.push(stack);
    }

    let pattern = order_pattern(context, count);
    let contextual = (!pattern.is_empty() && multiplier(&contents, &pattern) > 0)
        .then(|| {
            device
                .recipes
                .iter()
                .position(|r| assign(&r.ingredients, &pattern).is_some())
        })
        .flatten();

    if !existing.is_empty() {
        let servings = multiplier(&contents, &existing);
        if servings == 0 {
            return None;
        }
        for (index, recipe) in device.recipes.iter().enumerate() {
            if contextual.is_some_and(|selected| selected != index) {
                continue;
            }
            if assign(&recipe.ingredients, &existing).is_some() {
                return finish(
                    start,
                    &before,
                    Some(&occupied),
                    &existing,
                    servings,
                    accepts_recipe(recipe),
                );
            }
        }
        return None;
    }

    if let Some(index) = contextual {
        return finish(
            start,
            &before,
            None,
            &pattern,
            multiplier(&contents, &pattern),
            accepts_recipe(&device.recipes[index]),
        );
    }

    let nonempty: Vec<Stack> = items.iter().filter(|s| !s.is_empty()).cloned().collect();
    if let Some(single) = units(&nonempty, 1, count) {
        for recipe in device.recipes {
            if assign(&recipe.ingredients, &single).is_some() {
                return finish(start, &before, None, &single, 1, accepts_recipe(recipe));
            }
        }
    }
    for recipe in device.recipes {
        let size = recipe.ingredients.len();
        if size == 0 || size > count || total % size as i64 != 0 {
            continue;
        }
        let servings = i32::try_from(total / size as i64).ok()?;
        if let Some(groups) = units(&contents, servings, count)
            && let Some(ordered) = assign(&recipe.ingredients, &groups)
        {
            return finish(
                start,
                &before,
                None,
                &ordered,
                servings,
                accepts_recipe(recipe),
            );
        }
    }
    None
}

fn finish(
    start: usize,
    before: &[Stack],
    occupied: Option<&[usize]>,
    layout: &[Stack],
    servings: i32,
    allowed: bool,
) -> Option<Plan> {
    if !allowed || servings <= 0 {
        return None;
    }
    let mut additions = vec![Stack::empty(); before.len()];
    for (index, stack) in layout.iter().enumerate() {
        let slot = occupied.map_or(index, |occupied| occupied[index]);
        additions[slot] = stack.with_count(servings);
    }
    Some(Plan {
        start,
        before: before.to_vec(),
        additions,
    })
}

fn aggregate(items: &[Stack]) -> Vec<Stack> {
    let mut result: Vec<Stack> = Vec::new();
    for stack in items.iter().filter(|stack| !stack.is_empty()) {
        if let Some(found) = result
            .iter_mut()
            .find(|other| other.is_same_item_same_components(stack))
        {
            let Some(count) = found.count.checked_add(stack.count) else {
                return Vec::new();
            };
            found.count = count;
        } else {
            result.push(stack.clone());
        }
    }
    result
}

fn order_pattern(context: Option<&[ContextEntry]>, limit: usize) -> Vec<Stack> {
    let Some(entries) = context else {
        return Vec::new();
    };
    let mut pattern = Vec::new();
    for entry in entries {
        if entry.stack.is_empty() {
            continue;
        }
        let Ok(count) = usize::try_from(entry.count) else {
            return Vec::new();
        };
        if count == 0 || count > limit.saturating_sub(pattern.len()) {
            return Vec::new();
        }
        pattern.extend(std::iter::repeat_n(entry.stack.one(), count));
    }
    pattern
}

fn multiplier(contents: &[Stack], layout: &[Stack]) -> i32 {
    let required = aggregate(layout);
    if required.is_empty() || required.len() != contents.len() {
        return 0;
    }
    let mut multiple = 0;
    for ingredient in &required {
        let Some(stack) = contents
            .iter()
            .find(|s| s.is_same_item_same_components(ingredient))
        else {
            return 0;
        };
        if stack.count % ingredient.count != 0 {
            return 0;
        }
        let ratio = stack.count / ingredient.count;
        if multiple != 0 && multiple != ratio {
            return 0;
        }
        multiple = ratio;
    }
    multiple
}

fn units(contents: &[Stack], servings: i32, limit: usize) -> Option<Vec<Stack>> {
    if servings <= 0 {
        return None;
    }
    let mut result = Vec::new();
    for stack in contents {
        if stack.count % servings != 0 {
            return None;
        }
        let repeats = usize::try_from(stack.count / servings).ok()?;
        if repeats > limit.saturating_sub(result.len()) {
            return None;
        }
        result.extend(std::iter::repeat_n(stack.one(), repeats));
    }
    Some(result)
}

fn assign(ingredients: &[Ingredient], units: &[Stack]) -> Option<Vec<Stack>> {
    if ingredients.is_empty() || ingredients.len() != units.len() || units.len() > 9 {
        return None;
    }
    let mut ordered = vec![None; units.len()];
    let mut failed = vec![false; 1 << units.len()];
    fn search(
        ingredients: &[Ingredient],
        units: &[Stack],
        ordered: &mut [Option<Stack>],
        used: usize,
        failed: &mut [bool],
    ) -> bool {
        let index = used.count_ones() as usize;
        if index == units.len() {
            return true;
        }
        if failed[used] {
            return false;
        }
        for (i, stack) in units.iter().enumerate() {
            if (used & (1 << i)) != 0 || !ingredients[index].test(stack) {
                continue;
            }
            ordered[index] = Some(stack.clone());
            if search(ingredients, units, ordered, used | (1 << i), failed) {
                return true;
            }
        }
        failed[used] = true;
        false
    }
    search(ingredients, units, &mut ordered, 0, &mut failed)
        .then(|| ordered.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests;
