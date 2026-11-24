use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use fastnbt::Value;
use moddedmc_api::{Pos, content::World, tags::Tags};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::{
    RESOURCES, STACKS, TAGS,
    basin::Basin,
    belt_inventory::{TransportedItemStack, count, next_int, with_count},
    blaze_burner::Heat,
    fluids::pipe_connection::FluidStack,
    kinetics::Kinetics,
    press,
};

#[path = "sequenced_assembly.rs"]
mod sequenced_assembly;
use sequenced_assembly::{Advance, Assembly, Step};

pub enum FillingResult<'a> {
    Item(Value),
    Assembly(Advance<'a>),
}

impl FillingResult<'_> {
    pub fn finish(self) -> Value {
        match self {
            Self::Item(item) => item,
            Self::Assembly(step) => step.finish(),
        }
    }
}

pub enum ProcessingResult {
    Pass,
    Hold,
    Convert(Vec<Value>),
}

pub fn has_processor(kinetics: &Kinetics, pos: Pos) -> bool {
    let above = crate::offset(pos, [0, 2, 0]);
    kinetics.spouts.contains_key(&above) || kinetics.get(above).is_some_and(|be| be.press.is_some())
}

pub fn process_transported(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
) -> ProcessingResult {
    let above = crate::offset(pos, [0, 2, 0]);
    if let Some(spout) = kinetics.spouts.get_mut(&above) {
        return spout.process(world, above, item);
    }
    press::process_item(kinetics, pos, item)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Process {
    Pressing,
    Splashing,
    Haunting,
    Smoking,
    Smelting,
    Blasting,
    Milling,
    Crushing,
}

impl Process {
    const fn from_type(id: &str) -> Option<Self> {
        match id.as_bytes() {
            b"create:pressing" => Some(Self::Pressing),
            b"create:splashing" => Some(Self::Splashing),
            b"create:haunting" => Some(Self::Haunting),
            b"minecraft:smoking" => Some(Self::Smoking),
            b"minecraft:smelting" => Some(Self::Smelting),
            b"minecraft:blasting" => Some(Self::Blasting),
            b"create:milling" => Some(Self::Milling),
            b"create:crushing" => Some(Self::Crushing),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct RecipeBook {
    by_item: HashMap<(Process, String), Vec<Recipe>>,
    basin: Vec<BasinRecipe>,
    filling: Vec<FillingRecipe>,
    assemblies: Vec<Assembly>,
}

#[derive(Deserialize)]
struct RawRecipe {
    #[serde(rename = "type")]
    recipe_type: String,
    #[serde(default)]
    ingredients: Vec<Json>,
    ingredient: Option<Json>,
    #[serde(default)]
    results: Vec<ResultStack>,
    result: Option<ResultStack>,
    #[serde(default, rename = "processing_time")]
    duration: i32,
    heat_requirement: Option<String>,
}

#[derive(Clone, Deserialize)]
struct ResultStack {
    id: String,
    #[serde(default = "one")]
    count: i32,
    #[serde(default)]
    components: Option<Json>,
    #[serde(default = "certain")]
    chance: f32,
    amount: Option<u32>,
}

const fn certain() -> f32 {
    1.0
}

const fn one() -> i32 {
    1
}

#[derive(Clone)]
pub struct Recipe {
    outputs: Vec<Value>,
    chances: Vec<f32>,
    pub duration: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BasinProcess {
    Mixing,
    Compacting,
}

struct BasinRecipe {
    id: String,
    process: BasinProcess,
    minimum_heat: Heat,
    items: Vec<HashSet<String>>,
    fluids: Vec<(HashSet<String>, u32)>,
    fluid_results: Vec<FluidStack>,
    recipe: Recipe,
}

impl BasinRecipe {
    fn consume(&self, basin: &mut Basin) -> bool {
        self.items
            .iter()
            .all(|items| basin.consume(|item| items.contains(item)))
            && self
                .fluids
                .iter()
                .all(|(ids, amount)| basin.consume_fluid(|fluid| ids.contains(&fluid.id), *amount))
    }

    fn store_results(&self, basin: &mut Basin, items: &[Value]) -> bool {
        items.iter().all(|item| basin.put_result(item))
            && self
                .fluid_results
                .iter()
                .all(|fluid| basin.put_fluid_result(fluid))
    }
}

struct FillingRecipe {
    items: HashSet<String>,
    fluids: HashSet<String>,
    amount: u32,
    output: Value,
}

static BOOK: Mutex<Option<(u64, Arc<RecipeBook>)>> = Mutex::new(None);

pub fn current() -> Option<Arc<RecipeBook>> {
    let resources = *RESOURCES.get()?;
    let generation = resources.generation();
    if generation == 0 {
        return None;
    }
    let mut cached = BOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((previous, recipes)) = cached.as_ref()
        && *previous == generation
    {
        return Some(Arc::clone(recipes));
    }
    let recipes = Arc::new(load(resources)?);
    *cached = Some((generation, Arc::clone(&recipes)));
    Some(recipes)
}

fn load(resources: moddedmc_api::resources::Resources) -> Option<RecipeBook> {
    let mut book = RecipeBook::default();
    let mut tags = HashMap::<String, Vec<String>>::new();
    let tag_api = *TAGS.get()?;
    for path in resources.list("recipe")? {
        if path.ends_with("_manual_only.json") {
            continue;
        }
        let Some(bytes) = resources.read(&path) else {
            continue;
        };
        let Ok(raw) = serde_json::from_slice::<RawRecipe>(&bytes) else {
            continue;
        };
        let mut values = |tag: &str| {
            tags.entry(tag.to_owned())
                .or_insert_with(|| tag_api.values("item", tag).unwrap_or_default())
                .clone()
        };
        match raw.recipe_type.as_str() {
            "create:mixing" => {
                book.add_basin(path, BasinProcess::Mixing, raw, &mut values, tag_api)
            }
            "create:compacting" => {
                book.add_basin(path, BasinProcess::Compacting, raw, &mut values, tag_api)
            }
            "create:filling" => book.add_filling(raw, &mut values, tag_api),
            "create:sequenced_assembly" => {
                book.assemblies
                    .extend(Assembly::parse(&path, &bytes, &mut values, tag_api));
            }
            _ => book.add(raw, values),
        }
    }
    book.basin
        .sort_by_key(|recipe| std::cmp::Reverse(recipe.items.len()));
    Some(book)
}

impl RecipeBook {
    fn add(&mut self, raw: RawRecipe, mut tag_values: impl FnMut(&str) -> Vec<String>) {
        let Some(kind) = Process::from_type(&raw.recipe_type) else {
            return;
        };
        let ingredient = match raw.ingredients.as_slice() {
            [one] => one,
            [] if let Some(one) = &raw.ingredient => one,
            _ => return,
        };
        let mut items = HashSet::new();
        read_ingredient(ingredient, &mut tag_values, &mut items);
        let results = if let Some(result) = raw.result {
            vec![result]
        } else {
            raw.results
        };
        if items.is_empty() || results.is_empty() {
            return;
        }
        let chances = results.iter().map(|result| result.chance).collect();
        let outputs: Option<Vec<_>> = results.into_iter().map(ResultStack::into_nbt).collect();
        let Some(outputs) = outputs else {
            return;
        };
        let recipe = Recipe {
            outputs,
            chances,
            duration: raw.duration,
        };
        for item in items {
            self.by_item
                .entry((kind, item))
                .or_default()
                .push(recipe.clone());
        }
    }

    fn add_basin(
        &mut self,
        id: String,
        process: BasinProcess,
        raw: RawRecipe,
        tags: &mut impl FnMut(&str) -> Vec<String>,
        tag_api: Tags,
    ) {
        let minimum_heat = match raw.heat_requirement.as_deref() {
            None | Some("none") => Heat::None,
            Some("heated") => Heat::Fading,
            Some("superheated") => Heat::Seething,
            _ => return,
        };
        if !(1..=11).contains(&raw.ingredients.len()) || raw.results.is_empty() {
            return;
        }
        let mut items = Vec::new();
        let mut fluids = Vec::new();
        for ingredient in &raw.ingredients {
            let kind = ingredient.get("type").and_then(Json::as_str);
            let tag = ingredient.get("tag").and_then(Json::as_str);
            if matches!(kind, Some("neoforge:single" | "neoforge:tag")) {
                let Some(fluid) = read_fluid_ingredient(ingredient, tag_api) else {
                    return;
                };
                fluids.push(fluid);
                continue;
            }
            let mut accepted = HashSet::new();
            if kind == Some("neoforge:block_tag")
                && let Some(tag) = tag
            {
                accepted.extend(tag_api.values("block", tag).unwrap_or_default());
            } else {
                read_ingredient(ingredient, tags, &mut accepted);
            }
            if accepted.is_empty() {
                return;
            }
            items.push(accepted);
        }
        if items.len() > 9 || fluids.len() > 2 {
            return;
        }
        let mut outputs = Vec::new();
        let mut chances = Vec::new();
        let mut fluid_results = Vec::new();
        for result in raw.results {
            if let Some(amount) = result.amount {
                if amount == 0 || result.components.is_some() {
                    return;
                }
                fluid_results.push(FluidStack {
                    id: canonical_id(&result.id),
                    amount,
                    components: None,
                });
            } else {
                chances.push(result.chance);
                let Some(item) = result.into_nbt() else {
                    return;
                };
                outputs.push(item);
            }
        }
        self.basin.push(BasinRecipe {
            id,
            process,
            minimum_heat,
            items,
            fluids,
            fluid_results,
            recipe: Recipe {
                outputs,
                chances,
                duration: raw.duration,
            },
        });
    }

    pub fn matching_basin(&self, process: BasinProcess, basin: &Basin, heat: Heat) -> Option<&str> {
        self.basin.iter().find_map(|recipe| {
            (recipe.process == process && Self::can_process_basin(recipe, basin, heat))
                .then_some(recipe.id.as_str())
        })
    }

    fn can_process_basin(recipe: &BasinRecipe, basin: &Basin, heat: Heat) -> bool {
        if heat < recipe.minimum_heat {
            return false;
        }
        let mut staged = basin.clone();
        recipe.consume(&mut staged) && recipe.store_results(&mut staged, &recipe.recipe.outputs)
    }

    pub fn basin_duration(&self, id: &str) -> Option<i32> {
        self.basin
            .iter()
            .find(|recipe| recipe.id == id)
            .map(|recipe| recipe.recipe.duration)
    }

    pub fn can_continue_basin(&self, id: &str, basin: &Basin, heat: Heat) -> bool {
        self.basin
            .iter()
            .find(|recipe| recipe.id == id)
            .is_some_and(|recipe| Self::can_process_basin(recipe, basin, heat))
    }

    pub fn apply_basin(&self, id: &str, basin: &Basin, heat: Heat) -> Option<Basin> {
        let recipe = self
            .basin
            .iter()
            .find(|recipe| recipe.id == id && heat >= recipe.minimum_heat)?;
        let mut staged = basin.clone();
        (recipe.consume(&mut staged)
            && recipe.store_results(&mut staged, &recipe.recipe.roll_results(1)))
        .then_some(staged)
    }

    fn add_filling(
        &mut self,
        raw: RawRecipe,
        tags: &mut impl FnMut(&str) -> Vec<String>,
        tag_api: Tags,
    ) {
        let [item, fluid] = raw.ingredients.as_slice() else {
            return;
        };
        let [result] = raw.results.as_slice() else {
            return;
        };
        let Some((fluids, amount)) = read_fluid_ingredient(fluid, tag_api) else {
            return;
        };
        let Some(output) = result.clone().into_nbt() else {
            return;
        };
        let mut items = HashSet::new();
        read_ingredient(item, tags, &mut items);
        if !items.is_empty() && is_registered_output(&output) {
            self.filling.push(FillingRecipe {
                items,
                fluids,
                amount,
                output,
            });
        }
    }

    pub fn can_fill(&self, item: &Value) -> bool {
        self.assemblies
            .iter()
            .any(|assembly| matches!(assembly.next(item), Some((Step::Fill { .. }, _))))
            || item_id(item)
                .is_some_and(|id| self.filling.iter().any(|recipe| recipe.items.contains(id)))
    }

    pub fn fill(&self, item: &Value, fluid: &FluidStack) -> Option<(u32, FillingResult<'_>)> {
        for assembly in &self.assemblies {
            if let Some((Step::Fill { fluids, amount }, advance)) = assembly.next(item)
                && fluids.contains(&fluid.id)
            {
                return Some((*amount, FillingResult::Assembly(advance)));
            }
        }
        let id = item_id(item)?;
        self.filling.iter().find_map(|recipe| {
            (recipe.items.contains(id) && recipe.fluids.contains(&fluid.id))
                .then(|| (recipe.amount, FillingResult::Item(recipe.output.clone())))
        })
    }

    fn assembly_press(&self, item: &Value) -> Option<Advance<'_>> {
        self.assemblies
            .iter()
            .find_map(|assembly| match assembly.next(item) {
                Some((Step::Press, step)) => Some(step),
                _ => None,
            })
    }

    pub fn can_press(&self, item: &Value) -> bool {
        self.assembly_press(item).is_some() || self.find(Process::Pressing, item).is_some()
    }

    pub fn press(&self, item: &Value) -> Option<Vec<Value>> {
        if let Some(step) = self.assembly_press(item) {
            return Some(vec![step.finish()]);
        }
        self.find(Process::Pressing, item).map(Recipe::outputs)
    }

    pub fn find(&self, process: Process, item: &Value) -> Option<&Recipe> {
        let key = (process, item_id(item)?.to_owned());
        self.by_item
            .get(&key)?
            .iter()
            .find(|recipe| recipe.outputs.iter().all(is_registered_output))
    }
}

fn is_registered_output(output: &Value) -> bool {
    fastnbt::to_bytes(output)
        .ok()
        .and_then(|bytes| STACKS.get()?.info(&bytes))
        .is_some_and(|info| info.count > 0)
}

fn read_fluid_ingredient(ingredient: &Json, tags: Tags) -> Option<(HashSet<String>, u32)> {
    let kind = ingredient.get("type")?.as_str()?;
    let ids: HashSet<_> = match kind {
        "neoforge:single" => HashSet::from([canonical_id(ingredient.get("fluid")?.as_str()?)]),
        "neoforge:tag" => tags
            .values("fluid", ingredient.get("tag")?.as_str()?)?
            .into_iter()
            .collect(),
        _ => return None,
    };
    let amount = u32::try_from(ingredient.get("amount")?.as_u64()?).ok()?;
    (!ids.is_empty() && amount > 0).then_some((ids, amount))
}

impl ResultStack {
    fn into_nbt(self) -> Option<Value> {
        if !(1..=99).contains(&self.count) || self.components.is_some() || self.amount.is_some() {
            return None;
        }
        Some(Value::Compound(HashMap::from([
            ("id".into(), Value::String(canonical_id(&self.id))),
            ("count".into(), Value::Int(self.count)),
        ])))
    }
}

impl Recipe {
    pub fn outputs(&self) -> Vec<Value> {
        self.outputs.clone()
    }

    pub fn first_output(&self) -> Option<&str> {
        item_id(self.outputs.first()?)
    }

    /// RecipeApplier rolls each input independently and stacks the products.
    pub fn roll_results(&self, inputs: i32) -> Vec<Value> {
        let mut products: Vec<Value> = Vec::new();
        for _ in 0..inputs {
            for (output, &chance) in self.outputs.iter().zip(&self.chances) {
                if chance < 1.0 && (next_int(1 << 24) as f32) / (1 << 24) as f32 >= chance {
                    continue;
                }
                let max = fastnbt::to_bytes(output)
                    .ok()
                    .and_then(|bytes| STACKS.get()?.info(&bytes))
                    .map_or(64, |info| info.max_count.max(1) as i32);
                let mut remaining = count(output);
                for existing in &mut products {
                    if with_count(existing, 1) == with_count(output, 1) {
                        let old = count(existing);
                        let taken = (max - old).max(0).min(remaining);
                        *existing = with_count(existing, old + taken);
                        remaining -= taken;
                    }
                }
                while remaining > 0 {
                    let taken = max.min(remaining);
                    products.push(with_count(output, taken));
                    remaining -= taken;
                }
            }
        }
        products
    }
}

pub fn item_id(item: &Value) -> Option<&str> {
    let Value::Compound(values) = item else {
        return None;
    };
    match values.get("id") {
        Some(Value::String(id)) => Some(id),
        _ => None,
    }
}

fn canonical_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

fn read_ingredient(
    value: &Json,
    tag_values: &mut impl FnMut(&str) -> Vec<String>,
    items: &mut HashSet<String>,
) {
    match value {
        Json::String(id) => {
            if let Some(tag) = id.strip_prefix('#') {
                items.extend(tag_values(tag));
            } else {
                items.insert(canonical_id(id));
            }
        }
        Json::Array(values) => {
            for value in values {
                read_ingredient(value, tag_values, items);
            }
        }
        Json::Object(value) => {
            if let Some(id) = value.get("item").and_then(Json::as_str) {
                items.insert(canonical_id(id));
            } else if let Some(tag) = value.get("tag").and_then(Json::as_str) {
                items.extend(tag_values(tag));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexing_recipe_reads_merged_tag_members_and_keeps_recipe_priority() {
        let mut book = RecipeBook::default();
        for result in ["create:iron_sheet", "minecraft:paper"] {
            let raw: RawRecipe = serde_json::from_value(serde_json::json!({
                "type": "create:pressing",
                "ingredients": [[{"tag":"c:ingots/iron"}, "minecraft:sugar_cane"]],
                "results": [{"id":result}],
            }))
            .expect("recipe data");
            book.add(raw, |tag| {
                assert_eq!(tag, "c:ingots/iron");
                vec!["minecraft:iron_ingot".into()]
            });
        }
        for item in ["minecraft:iron_ingot", "minecraft:sugar_cane"] {
            let recipes = &book.by_item[&(Process::Pressing, item.into())];
            assert_eq!(item_id(&recipes[0].outputs[0]), Some("create:iron_sheet"));
            assert_eq!(item_id(&recipes[1].outputs[0]), Some("minecraft:paper"));
        }
    }
}
