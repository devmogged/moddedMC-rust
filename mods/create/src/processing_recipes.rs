use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use fastnbt::Value;
use serde::Deserialize;
use serde_json::Value as Json;

use crate::{RESOURCES, STACKS, TAGS};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Process {
    Pressing,
}

impl Process {
    const fn from_type(id: &str) -> Option<Self> {
        match id.as_bytes() {
            b"create:pressing" => Some(Self::Pressing),
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
    ingredients: Vec<Json>,
    results: Vec<ResultStack>,
}

#[derive(Clone, Deserialize)]
struct ResultStack {
    id: String,
    #[serde(default = "one")]
    count: i32,
    #[serde(default)]
    components: Option<Json>,
}

const fn one() -> i32 {
    1
}

#[derive(Clone)]
pub struct Recipe {
    outputs: Vec<Value>,
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
        let [ingredient] = raw.ingredients.as_slice() else {
            return;
        };
        let mut items = HashSet::new();
        read_ingredient(ingredient, &mut tag_values, &mut items);
        if items.is_empty() || raw.results.is_empty() {
            return;
        }
        let outputs: Option<Vec<_>> = raw.results.into_iter().map(ResultStack::into_nbt).collect();
        let Some(outputs) = outputs else {
            return;
        };
        for item in items {
            self.by_item.entry((kind, item)).or_default().push(Recipe {
                outputs: outputs.clone(),
            });
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
        if self.count <= 0 || self.components.is_some() {
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
