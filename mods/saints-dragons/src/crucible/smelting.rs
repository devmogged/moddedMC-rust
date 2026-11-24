use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::resources::Resources;
use serde::Deserialize;

use crate::{RESOURCES, TAGS};

use super::{Stack, stack};

type RecipeIndex = HashMap<String, (Stack, i32)>;
static RECIPES: Mutex<Option<(u64, RecipeIndex)>> = Mutex::new(None);

#[derive(Deserialize)]
struct Recipe {
    #[serde(rename = "type")]
    kind: String,
    ingredient: Ingredient,
    result: ResultStack,
    #[serde(default = "default_time")]
    cookingtime: i32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Ingredient {
    Id(String),
    Any(Vec<Ingredient>),
}

#[derive(Deserialize)]
struct ResultStack {
    id: String,
    #[serde(default = "one")]
    count: i32,
    components: Option<serde_json::Value>,
}

const fn one() -> i32 {
    1
}

const fn default_time() -> i32 {
    200
}

impl Ingredient {
    fn collect(self, output: &mut Vec<String>) {
        match self {
            Self::Id(id) => {
                if let Some(tag) = id.strip_prefix('#') {
                    if let Some(values) = TAGS.get().and_then(|tags| tags.values("item", tag)) {
                        output.extend(values);
                    }
                } else {
                    output.push(id);
                }
            }
            Self::Any(ingredients) => {
                for ingredient in ingredients {
                    ingredient.collect(output);
                }
            }
        }
    }
}

fn load(resources: Resources) -> Option<RecipeIndex> {
    let mut recipes = RecipeIndex::new();
    for path in resources.list("recipe")? {
        let Some(bytes) = resources.read(&path) else {
            continue;
        };
        let Ok(recipe) = serde_json::from_slice::<Recipe>(&bytes) else {
            continue;
        };
        if recipe.kind != "minecraft:smelting"
            || recipe.cookingtime <= 0
            || !(1..=99).contains(&recipe.result.count)
            || recipe.result.components.is_some()
        {
            continue;
        }
        let mut items = Vec::new();
        recipe.ingredient.collect(&mut items);
        for id in items {
            recipes.entry(id).or_insert_with(|| {
                (
                    stack(&recipe.result.id, recipe.result.count),
                    recipe.cookingtime,
                )
            });
        }
    }
    Some(recipes)
}

pub fn find(id: &str) -> Option<(Stack, i32)> {
    let resources = *RESOURCES.get()?;
    let generation = resources.generation();
    if generation == 0
        || TAGS.get().is_some_and(|tags| {
            tags.contains(
                "item",
                "saintsdragons:draconic_crucible/vanilla_smelting_blacklist",
                id,
            )
        })
    {
        return None;
    }
    let mut cache = RECIPES.lock().unwrap_or_else(PoisonError::into_inner);
    if cache
        .as_ref()
        .is_none_or(|(previous, _)| *previous != generation)
    {
        *cache = Some((generation, load(resources)?));
    }
    cache.as_ref()?.1.get(id).cloned()
}
