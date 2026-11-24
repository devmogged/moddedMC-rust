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
    fluids::pipe_connection::FluidStack,
    kinetics::Kinetics,
    press,
};

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
    mixing: Vec<MixingRecipe>,
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

struct MixingRecipe {
    id: String,
    items: Vec<HashSet<String>>,
    fluids: Vec<(HashSet<String>, u32)>,
    fluid_results: Vec<FluidStack>,
    recipe: Recipe,
}

impl MixingRecipe {
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
        if raw.recipe_type == "create:mixing" {
            book.add_mixing(path, raw, &mut values, tag_api);
        } else {
            book.add(raw, values);
        }
    }
    book.mixing
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

    fn add_mixing(
        &mut self,
        id: String,
        raw: RawRecipe,
        tags: &mut impl FnMut(&str) -> Vec<String>,
        tag_api: Tags,
    ) {
        if raw
            .heat_requirement
            .as_deref()
            .is_some_and(|heat| heat != "none")
            || !(1..=11).contains(&raw.ingredients.len())
            || raw.results.is_empty()
        {
            return;
        }
        let mut items = Vec::new();
        let mut fluids = Vec::new();
        for ingredient in &raw.ingredients {
            let kind = ingredient.get("type").and_then(Json::as_str);
            let tag = ingredient.get("tag").and_then(Json::as_str);
            if matches!(kind, Some("neoforge:single" | "neoforge:tag")) {
                let ids: HashSet<String> =
                    if let Some(id) = ingredient.get("fluid").and_then(Json::as_str) {
                        HashSet::from([canonical_id(id)])
                    } else if let Some(tag) = tag {
                        tag_api
                            .values("fluid", tag)
                            .unwrap_or_default()
                            .into_iter()
                            .collect()
                    } else {
                        return;
                    };
                let Some(amount) = ingredient
                    .get("amount")
                    .and_then(Json::as_u64)
                    .and_then(|amount| u32::try_from(amount).ok())
                    .filter(|amount| *amount > 0)
                else {
                    return;
                };
                if ids.is_empty() {
                    return;
                }
                fluids.push((ids, amount));
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
        self.mixing.push(MixingRecipe {
            id,
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

    pub fn matching_mixing(&self, basin: &Basin) -> Option<&str> {
        self.mixing
            .iter()
            .find_map(|recipe| Self::can_mix(recipe, basin).then_some(recipe.id.as_str()))
    }

    fn can_mix(recipe: &MixingRecipe, basin: &Basin) -> bool {
        let mut staged = basin.clone();
        recipe.consume(&mut staged) && recipe.store_results(&mut staged, &recipe.recipe.outputs)
    }

    pub fn mixing_duration(&self, id: &str) -> Option<i32> {
        self.mixing
            .iter()
            .find(|recipe| recipe.id == id)
            .map(|recipe| recipe.recipe.duration)
    }

    pub fn can_continue_mixing(&self, id: &str, basin: &Basin) -> bool {
        self.mixing
            .iter()
            .find(|recipe| recipe.id == id)
            .is_some_and(|recipe| Self::can_mix(recipe, basin))
    }

    pub fn apply_mixing(&self, id: &str, basin: &Basin) -> Option<Basin> {
        let recipe = self.mixing.iter().find(|recipe| recipe.id == id)?;
        let mut staged = basin.clone();
        (recipe.consume(&mut staged)
            && recipe.store_results(&mut staged, &recipe.recipe.roll_results(1)))
        .then_some(staged)
    }

    pub fn find(&self, process: Process, item: &Value) -> Option<&Recipe> {
        let key = (process, item_id(item)?.to_owned());
        self.by_item.get(&key)?.iter().find(|recipe| {
            recipe.outputs.iter().all(|output| {
                fastnbt::to_bytes(output)
                    .ok()
                    .and_then(|bytes| STACKS.get()?.info(&bytes))
                    .is_some_and(|info| info.count > 0)
            })
        })
    }
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
