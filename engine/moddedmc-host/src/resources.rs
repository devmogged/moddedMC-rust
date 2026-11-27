//! `moddedmc:resources` v1: the merged datapack files.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use moddedmc_api::abi::{self, MAX_RESOURCE_BYTES, Sink, Str};
use pumpkin_core::data::datapack::LoadedDatapack;
use pumpkin_data::recipes::{
    CookingRecipe, CookingRecipeType, RECIPES_COOKING, RECIPES_STONECUTTING, RecipeCategoryTypes,
    RecipeIngredientTypes, RecipeResultStruct, StonecutterRecipe,
};

use crate::native::SERVER;

/// How deep `list` walks below the directory; vanilla has no limit, real packs stay far below it.
const MAX_DEPTH: usize = 16;
/// Ids one `list` call returns, so a huge pack can't stall the tick indefinitely.
const MAX_LISTED: usize = 1 << 16;

/// Vanilla `ResourceLocation.isAllowedInResourceLocation` for paths.
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn valid_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
}

/// Datapacks in priority order, highest last. `None` before they first load.
fn packs() -> Option<Vec<LoadedDatapack>> {
    let manager = &SERVER.get()?.datapack_manager;
    (manager.generation() != 0).then(|| manager.get_loaded_packs())
}

enum VanillaRecipe {
    Cooking(&'static CookingRecipeType),
    Stonecutting(&'static StonecutterRecipe),
}

impl VanillaRecipe {
    fn source(&self) -> String {
        match self {
            Self::Cooking(recipe) => cooking_json(recipe),
            Self::Stonecutting(recipe) => {
                let ingredient = ingredient_json(&recipe.ingredient);
                let result = result_json(&recipe.result);
                let group = group_json(recipe.group);
                format!(
                    r#"{{"type":"minecraft:stonecutting"{group},"ingredient":{ingredient},"result":{result}}}"#
                )
            }
        }
    }
}

/// Compiled vanilla recipes are the lowest-priority entries in the resource view.
fn vanilla_recipes() -> &'static BTreeMap<String, VanillaRecipe> {
    static RECIPES: OnceLock<BTreeMap<String, VanillaRecipe>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        RECIPES_COOKING
            .iter()
            .map(|recipe| (cooking(recipe).1.recipe_id, VanillaRecipe::Cooking(recipe)))
            .chain(
                RECIPES_STONECUTTING
                    .iter()
                    .map(|recipe| (recipe.recipe_id, VanillaRecipe::Stonecutting(recipe))),
            )
            .filter_map(|(id, recipe)| {
                let (namespace, path) = id.split_once(':')?;
                Some((format!("{namespace}:recipe/{path}.json"), recipe))
            })
            .collect()
    })
}

const fn cooking(recipe: &CookingRecipeType) -> (&'static str, &CookingRecipe) {
    match recipe {
        CookingRecipeType::Smelting(inner) => ("minecraft:smelting", inner),
        CookingRecipeType::Blasting(inner) => ("minecraft:blasting", inner),
        CookingRecipeType::Smoking(inner) => ("minecraft:smoking", inner),
        CookingRecipeType::CampfireCooking(inner) => ("minecraft:campfire_cooking", inner),
    }
}

/// The recipe as the JSON file vanilla ships in `data/minecraft/recipe`.
fn cooking_json(recipe: &CookingRecipeType) -> String {
    let (kind, recipe) = cooking(recipe);
    // Vanilla leaves out `misc`, the CookingBookCategory default.
    let category = match recipe.category {
        RecipeCategoryTypes::Food => ",\"category\":\"food\"",
        RecipeCategoryTypes::Blocks => ",\"category\":\"blocks\"",
        _ => "",
    };
    let ingredient = ingredient_json(&recipe.ingredient);
    let group = group_json(recipe.group);
    let result = result_json(&recipe.result);
    format!(
        r#"{{"type":"{kind}"{category},"cookingtime":{},"experience":{}{group},"ingredient":{ingredient},"result":{result}}}"#,
        recipe.cookingtime, recipe.experience
    )
}

fn ingredient_json(ingredient: &RecipeIngredientTypes) -> String {
    match ingredient {
        RecipeIngredientTypes::Simple(id) | RecipeIngredientTypes::Tagged(id) => {
            format!(r#""{id}""#)
        }
        RecipeIngredientTypes::OneOf(ids) => {
            let ids: Vec<_> = ids.iter().map(|id| format!(r#""{id}""#)).collect();
            format!("[{}]", ids.join(","))
        }
    }
}

fn group_json(group: Option<&str>) -> String {
    group.map_or_else(String::new, |group| {
        format!(
            r#","group":{group}"#,
            group = serde_json::Value::from(group)
        )
    })
}

fn result_json(result: &RecipeResultStruct) -> String {
    let count = match result.count {
        1 => String::new(),
        count => format!(r#","count":{count}"#),
    };
    format!(r#"{{"id":"{}"{count}}}"#, result.id)
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_LISTED {
            return;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let id = format!("{prefix}/{name}");
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth < MAX_DEPTH && valid_path(&name) {
                walk(&entry.path(), &id, depth + 1, out);
            }
        } else if kind.is_file() && valid_path(&name) {
            out.insert(id);
        }
    }
}

unsafe extern "C" fn list(directory: Str, out: Sink) -> bool {
    // SAFETY: the directory is borrowed for this call.
    let directory = unsafe { directory.str() };
    let (true, Some(packs)) = (valid_path(directory), packs()) else {
        return false;
    };
    let mut ids = BTreeSet::new();
    if directory == "recipe" {
        ids.extend(vanilla_recipes().keys().cloned());
    }
    for pack in &packs {
        let Ok(namespaces) = fs::read_dir(pack.root_path.join("data")) else {
            continue;
        };
        for namespace in namespaces.flatten() {
            let Ok(name) = namespace.file_name().into_string() else {
                continue;
            };
            if valid_namespace(&name) {
                let root = namespace.path().join(directory);
                walk(&root, &format!("{name}:{directory}"), 0, &mut ids);
            }
        }
    }
    for id in &ids {
        // SAFETY: the id is live until the synchronous sink returns.
        unsafe { (out.put)(out.ctx, 0, Str::new(id.as_bytes())) };
    }
    true
}

fn resolve(packs: &[LoadedDatapack], id: &str) -> Option<PathBuf> {
    let (namespace, path) = id.split_once(':')?;
    if !valid_namespace(namespace) || !valid_path(path) {
        return None;
    }
    packs.iter().rev().find_map(|pack| {
        let file = pack.root_path.join("data").join(namespace).join(path);
        file.is_file().then_some(file)
    })
}

unsafe extern "C" fn read(id: Str, out: Sink) -> bool {
    // SAFETY: the id is borrowed for this call.
    let id = unsafe { id.str() };
    let Some(packs) = packs() else {
        return false;
    };
    let Some(file) = resolve(&packs, id) else {
        let Some(recipe) = vanilla_recipes().get(id) else {
            return false;
        };
        let json = recipe.source();
        // SAFETY: the bytes are live until the synchronous sink returns.
        unsafe { (out.put)(out.ctx, 0, Str::new(json.as_bytes())) };
        return true;
    };
    let Ok(handle) = fs::File::open(file) else {
        return false;
    };
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_RESOURCE_BYTES).unwrap_or(u64::MAX);
    if handle.take(limit + 1).read_to_end(&mut bytes).is_err() || bytes.len() > MAX_RESOURCE_BYTES {
        return false;
    }
    // SAFETY: the bytes are live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn generation() -> u64 {
    SERVER
        .get()
        .map_or(0, |server| server.datapack_manager.generation())
}

pub static RESOURCES: abi::ResourcesV1 = abi::ResourcesV1 {
    size: size_of::<abi::ResourcesV1>(),
    list,
    read,
    generation,
};

#[cfg(test)]
mod tests {
    use serde_json::{Value, from_str};

    use super::vanilla_recipes;

    fn generated(id: &str) -> Option<Value> {
        let recipe = vanilla_recipes().get(id)?;
        from_str(&recipe.source()).ok()
    }

    /// Both expected files are copied from the 26.3 server jar.
    #[test]
    fn cooking_matches_vanilla_files() -> serde_json::Result<()> {
        let smoking: Value = from_str(
            r#"{"type":"minecraft:smoking","category":"food","cookingtime":200,"experience":0.35,
            "ingredient":"minecraft:potato","result":{"id":"minecraft:baked_potato"}}"#,
        )?;
        let charcoal: Value = from_str(
            r##"{"type":"minecraft:smelting","cookingtime":200,"experience":0.15,
            "ingredient":"#minecraft:logs_that_burn","result":{"id":"minecraft:charcoal"}}"##,
        )?;
        assert_eq!(
            generated("minecraft:recipe/baked_potato_from_smoking.json"),
            Some(smoking)
        );
        assert_eq!(generated("minecraft:recipe/charcoal.json"), Some(charcoal));
        Ok(())
    }

    #[test]
    fn stonecutting_matches_vanilla_file() -> serde_json::Result<()> {
        let slab: Value = from_str(
            r#"{"type":"minecraft:stonecutting","ingredient":"minecraft:stone",
            "result":{"count":2,"id":"minecraft:stone_slab"}}"#,
        )?;
        assert_eq!(
            generated("minecraft:recipe/stone_slab_from_stone_stonecutting.json"),
            Some(slab)
        );
        Ok(())
    }
}
