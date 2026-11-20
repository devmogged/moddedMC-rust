use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use fastnbt::Value;
use serde::Deserialize;
use serde_json::Value as Json;

use crate::{
    RESOURCES, STACKS, TAGS,
    belt_inventory::{count, next_int, with_count},
};

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
        book.add(raw, |tag| {
            tags.entry(tag.to_owned())
                .or_insert_with(|| tag_api.values("item", tag).unwrap_or_default())
                .clone()
        });
    }
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
        if !(1..=99).contains(&self.count) || self.components.is_some() {
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
