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
const NULLJAW: &str = "saintsdragons:nulljaw";
const NULLJAW_COOLDOWN: u64 = 24;
const NULLJAW_TAME_DENOMINATOR: u64 = 5;

// `setFeedingCooldown` per juvenile and Varasuchus, kept off disk like the adult cooldowns.
static FED: Mutex<Option<HashMap<i32, (u64, u64)>>> = Mutex::new(None);

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

/// Starts the feeding cooldown, or returns false while the dragon is still eating.
/// A zero duration only checks whether the dragon can feed.
pub(crate) fn start_eating(id: i32, cooldown: u64) -> bool {
    let mut fed = FED.lock().unwrap_or_else(PoisonError::into_inner);
    start_eating_at(fed.get_or_insert_default(), id, tick_count(), cooldown)
}

fn start_eating_at(fed: &mut HashMap<i32, (u64, u64)>, id: i32, now: u64, cooldown: u64) -> bool {
    fed.retain(|_, (_, until)| *until > now);
    if fed.contains_key(&id) {
        return false;
    }
    if cooldown > 0 {
        fed.insert(id, (now, now + cooldown));
    }
    true
}

/// The Varasuchus eating clip lasts two seconds, independently of the feeding cooldown.
pub fn eating_clip(id: i32) -> Option<&'static str> {
    let fed = FED.lock().unwrap_or_else(PoisonError::into_inner);
    let (started, _) = fed.as_ref()?.get(&id)?;
    (tick_count().saturating_sub(*started) < 40).then_some("eat")
}

/// Nulljaw.mobInteract: chorus fruit is ordinary food, and wild feeding rolls
/// one in five without starting a cooldown or applying feeding hunger.
fn nulljaw_wild_tame(roll: u64) -> bool {
    roll == 0
}

pub fn interact(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    let (player, mob, mobs) = (click.player?, click.mob?, MOBS.get()?);
    if !eggs::is_juvenile(mob) {
        return None;
    }
    let kind = mobs.kind(mob)?;
    let species = kind.strip_prefix("saintsdragons:")?;
    let nulljaw = kind == NULLJAW;
    let name = if nulljaw {
        crate::binders::saved_name(mob).unwrap_or_else(|| display(species))
    } else {
        display(species)
    };
    let tamed = taming::is_tamed(mob);
    let uuid = mobs.uuid(player)?;
    let tamed_cooldown = if nulljaw {
        NULLJAW_COOLDOWN
    } else if kind == crate::wild::VARASUCHUS {
        crate::wild::VARASUCHUS_FEED_COOLDOWN
    } else {
        TAMED_COOLDOWN
    };
    if nulljaw && player.vehicle() == Some(mob.id()) {
        return Some(UseResult::Pass);
    }
    if item == STUNTING_FOOD {
        // `tryStuntGrowth`: only the owner of a tamed juvenile.
        if !tamed || taming::owner(mob) != Some(uuid) {
            return None;
        }
        if eggs::is_stunted(mob) {
            mobs.message(player, &format!("{name} is not growing anymore"), true);
            return Some(UseResult::Success);
        }
        if !start_eating(mob.id(), tamed_cooldown) {
            mobs.message(player, &format!("{name} is still eating..."), true);
            return Some(UseResult::Consume);
        }
        if !click.creative {
            click.stack.shrink(1);
        }
        crate::audio::eat(click.world, mob, item);
        eggs::stunt(mob);
        crate::hunger::feed(mob, false);
        taming::feed_particles(click.world, mob, true);
        mobs.message(player, &format!("{name} will stay small forever"), true);
        return Some(UseResult::Success);
    }
    let food = (!nulljaw && item == taming::HEARTY_MEAL)
        || TAGS
            .get()?
            .contains("item", &format!("saintsdragons:foods/{species}"), item);
    if !food
        || (kind == crate::wild::VARASUCHUS
            && tamed
            && (click.sneaking || mobs.game_mode(player) == Some(3)))
    {
        return None;
    }
    if nulljaw {
        if !tamed {
            if !click.creative {
                click.stack.shrink(1);
            }
            crate::audio::eat(click.world, mob, item);
            let success = nulljaw_wild_tame(random(NULLJAW_TAME_DENOMINATOR))
                && taming::tame(click.world, mob, uuid);
            taming::feed_particles(click.world, mob, success);
            return Some(UseResult::Success);
        }
        if mobs.game_mode(player) == Some(3) {
            return Some(UseResult::Pass);
        }
        if click.sneaking {
            if !start_eating(mob.id(), 0) {
                mobs.message(player, &format!("{name} is still eating..."), true);
                return Some(UseResult::Consume);
            }
            mobs.message(player, &format!("{name} is too young to breed"), true);
            return Some(UseResult::Success);
        }
    }
    let hearty = !nulljaw && item == taming::HEARTY_MEAL;
    let wild = if tamed {
        None
    } else {
        Some(wild_chance(kind, item)?)
    };
    let cooldown = wild.map_or(tamed_cooldown, |(_, cooldown)| cooldown);
    if !start_eating(mob.id(), cooldown) {
        mobs.message(player, &format!("{name} is still eating..."), true);
        return Some(UseResult::Consume);
    }
    if !click.creative {
        click.stack.shrink(1);
    }
    crate::audio::eat(click.world, mob, item);
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
    let growth = eggs::feed_growth(mob, if hearty { GROWTH.1 } else { GROWTH.0 });
    let message = match growth {
        None => format!("{name} is not growing anymore"),
        Some(true) if species == "nulljaw" => format!("{name} has grown into an adult!"),
        Some(true) => format!("{name} has fully grown!"),
        Some(false) if matches!(species, "raevyx" | "ignivorus" | "nulljaw") => {
            format!("{name} is growing stronger!")
        }
        Some(false) => format!("{name} grows a bit stronger!"),
    };
    if !nulljaw || growth.is_some() {
        taming::feed_particles(click.world, mob, true);
    }
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
    fn varasuchus_feeding_and_breeding_share_cooldown() {
        let mut fed = HashMap::new();
        assert!(start_eating_at(&mut fed, 1, 100, 50));
        assert!(!start_eating_at(&mut fed, 1, 149, 0));
        assert!(!start_eating_at(&mut fed, 1, 149, 61));
        assert!(start_eating_at(&mut fed, 2, 149, 50));
        assert!(start_eating_at(&mut fed, 1, 150, 61));
        assert!(!start_eating_at(&mut fed, 1, 210, 50));
        assert!(start_eating_at(&mut fed, 1, 211, 50));
    }

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

    #[test]
    fn nulljaw_wild_feeding_has_one_success_in_five() {
        assert_eq!(
            (0..NULLJAW_TAME_DENOMINATOR)
                .filter(|&roll| nulljaw_wild_tame(roll))
                .count(),
            1
        );
        assert!(nulljaw_wild_tame(0));
    }
}
