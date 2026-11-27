//! The release's `minecraft:impossible` advancements, which 0.9.88 awards from code.

use moddedmc_api::{content::World, entities::Entity};

use crate::{ENTITIES, MOBS};

/// The released brewed_potion criteria use the corresponding custom potion holder.
/// Our recipes represent those two holders as standalone content items.
pub fn brewed_potion(_world: World<'_>, player: Entity<'_>, item: &str, _stack: &[u8]) {
    match item {
        "saintsdragons:potion_of_tideguard" => award(player, "brew_tideguard", "brew_tideguard"),
        "saintsdragons:potion_of_searing" => award(player, "brew_searing", "brew_searing"),
        _ => {}
    }
}

/// Grant `criterion` of `saintsdragons:<advancement>` to `player`.
pub fn award(player: Entity<'_>, advancement: &str, criterion: &str) {
    if let Some(mobs) = MOBS.get() {
        mobs.award_advancement(player, &format!("saintsdragons:{advancement}"), criterion);
    }
}

/// Award the player with `uuid` when they are in `world`. False when they aren't.
pub fn award_uuid(world: World<'_>, uuid: [u8; 16], advancement: &str, criterion: &str) -> bool {
    let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) else {
        return false;
    };
    mobs.by_uuid(world, &uuid)
        .and_then(|id| {
            entities.with_entity(world, id, |player| award(player, advancement, criterion))
        })
        .is_some()
}

/// Award every living player within `radius` of `at`: a sphere, or with `boxed` an axis-aligned
/// box inflated by `radius`.
pub fn award_near(
    world: World<'_>,
    at: [f64; 3],
    radius: f64,
    boxed: bool,
    advancement: &str,
    criterion: &str,
) {
    let Some(entities) = ENTITIES.get() else {
        return;
    };
    for id in entities.players(world) {
        entities.with_entity(world, id, |player| {
            let near = player.view().is_some_and(|view| {
                let offset = [0, 1, 2].map(|i| view.position[i] - at[i]);
                if boxed {
                    offset.iter().all(|d| d.abs() <= radius)
                } else {
                    offset.iter().map(|d| d * d).sum::<f64>() <= radius * radius
                }
            });
            let alive = MOBS
                .get()
                .and_then(|mobs| mobs.health(player))
                .is_some_and(|(health, _)| health > 0.0);
            if near && alive {
                award(player, advancement, criterion);
            }
        });
    }
}
