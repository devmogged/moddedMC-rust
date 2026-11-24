//! ManualApplicationRecipe for Create casings.

use moddedmc_api::{
    Pos,
    content::{self, BlockUpdate, World},
    effects::SOUND_PLAYERS,
    interaction::{UseOnBlock, UseResult},
};

use crate::{
    CONTENT, EFFECTS,
    processing_recipes::{self, ItemApplication, RecipeBook},
};

pub fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    if context.stack.count() == 0 || !context.player.is_some_and(|player| player.may_build()) {
        return UseResult::Pass;
    }
    let (world, position) = (context.world, context.position);
    let Some(recipes) = processing_recipes::current() else {
        return UseResult::Pass;
    };
    let Some(recipe) = recipe_at(world, position, &recipes, context.item) else {
        return UseResult::Pass;
    };
    if !apply(world, position, recipe) {
        return UseResult::Fail;
    }
    if !context.creative && !recipe.keep_held_item {
        context.stack.shrink(1);
    }
    UseResult::Success
}

pub fn recipe_at<'a>(
    world: World<'_>,
    position: Pos,
    recipes: &'a RecipeBook,
    ingredient: &str,
) -> Option<&'a ItemApplication> {
    let recipes = recipes.applications(ingredient);
    if recipes.is_empty() {
        return None;
    }
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[position], &mut state) {
        return None;
    }
    let item = CONTENT.get()?.block_item(state[0])?;
    recipes.iter().find(|recipe| recipe.matches(&item))
}

pub fn apply(world: World<'_>, position: Pos, recipe: &ItemApplication) -> bool {
    let Some(state) = CONTENT
        .get()
        .and_then(|content| content.state(&recipe.result))
    else {
        return false;
    };
    if world.write(&[BlockUpdate {
        position,
        state,
        flags: content::UPDATE_ALL,
    }]) != 1
    {
        return false;
    }
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            [position.x, position.y, position.z].map(|axis| f64::from(axis) + 0.5),
            "minecraft:block.copper.break",
            SOUND_PLAYERS,
            1.0,
            1.45,
        );
    }
    true
}
