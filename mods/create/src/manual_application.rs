//! ManualApplicationRecipe for Create casings.

use moddedmc_api::{
    content::{self, BlockUpdate},
    effects::SOUND_PLAYERS,
    interaction::{UseOnBlock, UseResult},
};

use crate::{CONTENT, EFFECTS, processing_recipes};

pub fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    if context.stack.count() == 0 || !context.player.is_some_and(|player| player.may_build()) {
        return UseResult::Pass;
    }
    let (world, position) = (context.world, context.position);
    let Some((content, recipes)) = CONTENT.get().zip(processing_recipes::current()) else {
        return UseResult::Pass;
    };
    let recipes = recipes.applications(context.item);
    if recipes.is_empty() {
        return UseResult::Pass;
    }
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[position], &mut state) {
        return UseResult::Pass;
    }
    let Some(item) = content.block_item(state[0]) else {
        return UseResult::Pass;
    };
    let Some(recipe) = recipes.iter().find(|recipe| recipe.matches(&item)) else {
        return UseResult::Pass;
    };
    let Some(state) = content.state(&recipe.result) else {
        return UseResult::Fail;
    };
    if world.write(&[BlockUpdate {
        position,
        state,
        flags: content::UPDATE_ALL,
    }]) != 1
    {
        return UseResult::Fail;
    }
    if !context.creative && !recipe.keep_held_item {
        context.stack.shrink(1);
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
    UseResult::Success
}
