//! `DragonBabyComponent` from 0.9.88: food taming for a wild juvenile, growth from feeding a
//! tamed one, and stunting a tamed juvenile's growth with raw moop.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    interaction::{UseMob, UseResult},
    tick_count,
};

use crate::{MOBS, TAGS, ai::random, eggs, taming};

const STUNTING_FOOD: &str = "saintsdragons:raw_moop";
/// `applyBabyGrowth(…, 2400, 4800)`, the same for every species.
const GROWTH: (i32, i32) = (2400, 4800);
const TAMED_COOLDOWN: u64 = 22;

// `setFeedingCooldown` per juvenile, kept off disk like the adult cooldowns.
static FED: Mutex<Option<HashMap<i32, u64>>> = Mutex::new(None);

fn display(species: &str) -> String {
    let mut name = species.to_owned();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    name
}

/// The wild juvenile's taming chance in percent and the cooldown its handler sets.
fn wild_chance(kind: &str, item: &str) -> Option<(f64, u64)> {
    let hearty = item == taming::HEARTY_MEAL;
    match kind {
        crate::CINDERVANE => Some((
            match item {
                _ if hearty => 50.0,
                "minecraft:chicken" => 33.3333,
                _ => 25.0,
            },
            61,
        )),
        "saintsdragons:stegonaut" => Some((100.0, 0)),
        "saintsdragons:varasuchus" => Some((
            match item {
                _ if hearty => 33.3334,
                "minecraft:tropical_fish" => 25.0,
                _ => 16.6667,
            },
            50,
        )),
        _ => crate::stun::taming_chance(kind, item),
    }
}

/// Starts the feeding cooldown, or returns false while the juvenile is still eating.
fn start_eating(id: i32, cooldown: u64) -> bool {
    let now = tick_count();
    let mut fed = FED.lock().unwrap_or_else(PoisonError::into_inner);
    let fed = fed.get_or_insert_default();
    fed.retain(|_, until| *until > now);
    if fed.contains_key(&id) {
        return false;
    }
    if cooldown > 0 {
        fed.insert(id, now + cooldown);
    }
    true
}

pub fn interact(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    let (player, mob, mobs) = (click.player?, click.mob?, MOBS.get()?);
    if !eggs::is_juvenile(mob) {
        return None;
    }
    let kind = mobs.kind(mob)?;
    let species = kind.strip_prefix("saintsdragons:")?;
    let name = display(species);
    let tamed = taming::is_tamed(mob);
    let uuid = mobs.uuid(player)?;
    if item == STUNTING_FOOD {
        // `tryStuntGrowth`: only the owner of a tamed juvenile.
        if !tamed || taming::owner(mob) != Some(uuid) {
            return None;
        }
        if eggs::is_stunted(mob) {
            mobs.message(player, &format!("{name} is not growing anymore"), true);
            return Some(UseResult::Success);
        }
        if !start_eating(mob.id(), TAMED_COOLDOWN) {
            mobs.message(player, &format!("{name} is still eating..."), true);
            return Some(UseResult::Consume);
        }
        if !click.creative {
            click.stack.shrink(1);
        }
        crate::audio::eat(click.world, mob);
        eggs::stunt(mob);
        crate::hunger::feed(mob, false);
        taming::feed_particles(click.world, mob, true);
        mobs.message(player, &format!("{name} will stay small forever"), true);
        return Some(UseResult::Success);
    }
    let food = item == taming::HEARTY_MEAL
        || TAGS
            .get()?
            .contains("item", &format!("saintsdragons:foods/{species}"), item);
    if !food {
        return None;
    }
    let hearty = item == taming::HEARTY_MEAL;
    let wild = if tamed {
        None
    } else {
        Some(wild_chance(kind, item)?)
    };
    let cooldown = wild.map_or(TAMED_COOLDOWN, |(_, cooldown)| cooldown);
    if !start_eating(mob.id(), cooldown) {
        mobs.message(player, &format!("{name} is still eating..."), true);
        return Some(UseResult::Consume);
    }
    if !click.creative {
        click.stack.shrink(1);
    }
    crate::audio::eat(click.world, mob);
    if hearty {
        mob.add_effect("minecraft:regeneration", 200, 1, false, true);
    }
    crate::hunger::feed(mob, hearty);
    if let Some((chance, _)) = wild {
        // `handleBabyFoodTaming`.
        let roll = random(1_000_000) as f64 / 10_000.0;
        let tamed = roll < chance && taming::tame(click.world, mob, uuid);
        taming::feed_particles(click.world, mob, tamed);
        if tamed && species == "stegonaut" {
            mobs.message(player, "Stegonaut has been tamed!", true);
        }
        return Some(UseResult::Success);
    }
    // `applyBabyGrowth`.
    let message = match eggs::feed_growth(mob, if hearty { GROWTH.1 } else { GROWTH.0 }) {
        None => format!("{name} is not growing anymore"),
        Some(true) if species == "nulljaw" => format!("{name} has grown into an adult!"),
        Some(true) => format!("{name} has fully grown!"),
        Some(false) if matches!(species, "raevyx" | "ignivorus" | "nulljaw") => {
            format!("{name} is growing stronger!")
        }
        Some(false) => format!("{name} grows a bit stronger!"),
    };
    taming::feed_particles(click.world, mob, true);
    mobs.message(player, &message, true);
    Some(UseResult::Success)
}

pub fn forget(id: i32) {
    if let Some(fed) = FED.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        fed.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn juvenile_chances_follow_each_handler() {
        assert_eq!(display("raevyx"), "Raevyx");
        assert_eq!(
            wild_chance(crate::CINDERVANE, taming::HEARTY_MEAL),
            Some((50.0, 61))
        );
        assert_eq!(
            wild_chance("saintsdragons:varasuchus", "minecraft:tropical_fish"),
            Some((25.0, 50))
        );
        assert_eq!(
            wild_chance("saintsdragons:stegonaut", "minecraft:wheat"),
            Some((100.0, 0))
        );
        assert_eq!(wild_chance("saintsdragons:nulljaw", "minecraft:beef"), None);
    }
}
