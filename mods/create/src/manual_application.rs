//! ManualApplicationRecipe for Create casings.

use moddedmc_api::{
    content::{self, BlockUpdate},
    effects::SOUND_PLAYERS,
    interaction::{UseOnBlock, UseResult},
};

use crate::{
    ANDESITE_ALLOY, BRASS_CASING, BRASS_INGOT, CASING, CONTENT, COPPER_CASING, EFFECTS, TAGS,
};

pub fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    let output = match context.item {
        ANDESITE_ALLOY => CASING,
        BRASS_INGOT => BRASS_CASING,
        "minecraft:copper_ingot" | "copper_ingot" => COPPER_CASING,
        _ => return UseResult::Pass,
    };
    if context.stack.count() == 0 || !context.player.is_some_and(|player| player.may_build()) {
        return UseResult::Pass;
    }
    let (world, position) = (context.world, context.position);
    let (Some(content), Some(tags)) = (CONTENT.get(), TAGS.get()) else {
        return UseResult::Pass;
    };
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[position], &mut state) {
        return UseResult::Pass;
    }
    let Some(item) = content.block_item(state[0]) else {
        return UseResult::Pass;
    };
    if !["c:stripped_logs", "c:stripped_woods"]
        .iter()
        .any(|tag| tags.contains("item", tag, &item))
    {
        return UseResult::Pass;
    }
    let Some(state) = content.state(output) else {
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
    if !context.creative {
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
