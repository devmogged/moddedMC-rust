//! Casing and wrench interactions for Create's shafts.

use moddedmc_api::{
    content::{BlockUpdate, UNLOADED_STATE, UPDATE_ALL},
    effects::SOUND_BLOCKS,
    interaction::{UseOnBlock, UseResult},
};

use crate::{
    AXES, BRASS_CASING, BRASS_ENCASED_SHAFT, CASING, EFFECTS, ENCASED_SHAFT, SHAFT, WRENCH,
    kinetics::Kind, states,
};

pub fn use_on_shaft(context: &UseOnBlock<'_>) -> Option<UseResult> {
    if !context.player.is_some_and(|player| player.may_build()) {
        return None;
    }
    let states = states()?;
    let pos = context.position;
    let mut current = [UNLOADED_STATE];
    context.world.read(&[pos], &mut current);
    let (selector, sound, pitch) = match (states.kind(current[0])?, context.item, context.sneaking)
    {
        (Kind::Shaft(axis), CASING | BRASS_CASING, false) => {
            let block = if context.item == CASING {
                ENCASED_SHAFT
            } else {
                BRASS_ENCASED_SHAFT
            };
            (
                format!("{block}[axis={}]", AXES[usize::from(axis)]),
                "minecraft:block.stone.place",
                0.8,
            )
        }
        (Kind::EncasedShaft(axis), WRENCH, true) => (
            format!(
                "{SHAFT}[axis={},waterlogged=false]",
                AXES[usize::from(axis)]
            ),
            "minecraft:block.stone.break",
            0.8,
        ),
        _ => return None,
    };
    let state = states.lookup(&selector)?;
    if context.world.write(&[BlockUpdate {
        position: pos,
        state,
        flags: UPDATE_ALL,
    }]) != 1
    {
        return Some(UseResult::Fail);
    }
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            context.world,
            [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5),
            sound,
            SOUND_BLOCKS,
            1.0,
            pitch,
        );
    }
    Some(UseResult::Success)
}
