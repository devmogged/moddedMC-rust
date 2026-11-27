//! 0.9.88 taming for the two species without a stun: Stegonaut takes food, and Varasuchus is
//! ridden until it either gives in or bucks the rider off.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{Entity, EntityChange, SET_MOTION},
    interaction::{UseMob, UseResult},
    mob_ai::goal_phase,
    tick_count,
};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, TAGS, ai::random, taming};

pub const GOAL: u64 = 14;
const STEGONAUT: &str = "saintsdragons:stegonaut";
pub const VARASUCHUS: &str = "saintsdragons:varasuchus";
const VARASUCHUS_FOODS: &str = "saintsdragons:foods/varasuchus";
pub(crate) const VARASUCHUS_FEED_COOLDOWN: u64 = 50;
/// `WILD_RIDE_BUCK_DURATION_TICKS` and `WILD_RIDE_BUCK_COOLDOWN_TICKS`.
const RIDE_TICKS: u64 = 90;
const BUCK_COOLDOWN: u64 = 100;
/// `varasuchus.json` `taming_chance`, in percent.
const RODEO_CHANCE: f64 = 16.6667;

#[derive(Default)]
struct State {
    /// The tick each wild ride started.
    rides: HashMap<i32, u64>,
    bucked: HashMap<i32, u64>,
}

// shortcut: a ride in progress lives in memory; a restart ends it with the rider still seated.
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default())
}

pub fn forget(id: i32) {
    with(|state| {
        state.rides.remove(&id);
        state.bucked.remove(&id);
    });
}

fn roll(percent: f64) -> bool {
    (random(1_000_000) as f64) / 10_000.0 < percent
}

fn wild_adult(mob: Entity<'_>) -> bool {
    !taming::is_tamed(mob) && !crate::eggs::is_juvenile(mob)
}

/// `VarasuchusInteractionHandler.handleUntamedInteraction`: all ordinary clicks
/// are refused during the existing roost's sleep and both transitions.
pub fn sleep_refusal(click: &UseMob<'_>) -> Option<UseResult> {
    let (player, mob, mobs) = (click.player?, click.mob?, MOBS.get()?);
    if mobs.kind(mob)? != VARASUCHUS
        || taming::is_tamed(mob)
        || crate::roost::sleep_clip(mob.id()).is_none()
    {
        return None;
    }
    let name = crate::binders::saved_name(mob).unwrap_or_else(|| "Varasuchus".to_owned());
    mobs.message(
        player,
        &format!("{name} is sleeping, wake it up to tame."),
        true,
    );
    Some(UseResult::Success)
}

fn feeding(hearty: bool, health: f32, max: f32, hungry: bool) -> (f32, &'static str) {
    let heal = if hearty { 35.0 } else { 10.0 };
    let message = if health + heal < max {
        "Varasuchus is being healed..."
    } else if hungry {
        "Feeding Varasuchus..."
    } else {
        "Varasuchus is fully healed!"
    };
    (heal, message)
}

/// `VarasuchusInteractionHandler.handleFeeding` for a tamed adult.
/// Crouching with food belongs to breeding, and any non-spectator may feed.
pub fn feed(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    let (player, mob, mobs) = (click.player?, click.mob?, MOBS.get()?);
    if mobs.kind(mob)? != VARASUCHUS
        || !taming::is_tamed(mob)
        || crate::eggs::is_juvenile(mob)
        || click.sneaking
        || mobs.game_mode(player) == Some(3)
        || !TAGS.get()?.contains("item", VARASUCHUS_FOODS, item)
    {
        return None;
    }
    let (health, max) = mobs.health(mob)?;
    if !crate::baby::start_eating(mob.id(), VARASUCHUS_FEED_COOLDOWN) {
        mobs.message(player, "Varasuchus is still eating...", true);
        return Some(UseResult::Consume);
    }
    if !click.creative {
        click.stack.shrink(1);
    }
    crate::audio::eat(click.world, mob);
    let hearty = item == taming::HEARTY_MEAL;
    let hungry = crate::hunger::feed(mob, hearty);
    let (heal, message) = feeding(hearty, health, max, hungry);
    mobs.heal(mob, heal);
    if hearty {
        mob.add_effect("minecraft:regeneration", 200, 1, false, true);
    }
    taming::feed_particles(click.world, mob, true);
    mobs.message(player, message, true);
    Some(UseResult::Success)
}

/// Stegonaut and Varasuchus clicks on a wild adult. None leaves the click to the other handlers.
pub fn interact(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    let (player, mob, mobs) = (click.player?, click.mob?, MOBS.get()?);
    let kind = mobs.kind(mob)?;
    if !wild_adult(mob) {
        return None;
    }
    if kind == STEGONAUT
        && TAGS
            .get()?
            .contains("item", "saintsdragons:foods/stegonaut", item)
    {
        // `StegonautInteractionHandler`: `taming_chance_base` and `_hearty` are both 100.
        if !click.creative {
            click.stack.shrink(1);
        }
        crate::audio::eat(click.world, mob);
        let tamed = roll(100.0) && taming::tame(click.world, mob, mobs.uuid(player)?);
        taming::feed_particles(click.world, mob, tamed);
        if tamed {
            mobs.message(player, "Stegonaut has been tamed!", true);
        }
        return Some(UseResult::Success);
    }
    // `beginUntamedRide`: an empty main hand, not crouching.
    if kind != VARASUCHUS || !item.is_empty() || !click.main_hand || click.sneaking {
        return None;
    }
    let now = tick_count();
    let cooldown = with(|state| state.bucked.get(&mob.id()).copied()).filter(|&until| until > now);
    if let Some(until) = cooldown {
        let seconds = (until - now).div_ceil(20).max(1);
        mobs.message(
            player,
            &format!("Nuh-uh. Varasuchus said no. Try again in {seconds} seconds."),
            true,
        );
        return Some(UseResult::Fail);
    }
    if !mobs.passengers(mob).is_empty() || !player.start_riding(mob.id(), false) {
        return Some(UseResult::Fail);
    }
    with(|state| state.rides.insert(mob.id(), now));
    if let Some(ai) = MOB_AI.get() {
        ai.stop(mob);
        ai.set_target(click.world, mob, None);
    }
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let sound = if cfg!(feature = "original-assets") {
            "saintsdragons:varasuchus_bucking"
        } else {
            "minecraft:entity.ravager.roar"
        };
        effects.play_sound(click.world, view.position, sound, SOUND_NEUTRAL, 1.0, 1.0);
    }
    Some(UseResult::Success)
}

/// `tickWildRideState`, as a goal that holds the Varasuchus still while it bucks.
pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let Some(started) = with(|state| state.rides.get(&mob.id()).copied()) else {
        return false;
    };
    let (Some(mobs), Some(ai), Some(entities)) = (MOBS.get(), MOB_AI.get(), ENTITIES.get()) else {
        return false;
    };
    let rider = mobs.passengers(mob).first().copied();
    if taming::is_tamed(mob) || !wild_adult(mob) || rider.is_none() {
        forget_ride(mob);
        return false;
    }
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => true,
        goal_phase::START | goal_phase::TICK => {
            ai.stop(mob);
            mob.reset_fall_distance();
            let Some(rider) = rider else {
                return false;
            };
            if tick_count() < started + RIDE_TICKS {
                entities.with_entity(world, rider, |rider| rider.reset_fall_distance());
                return false;
            }
            forget_ride(mob);
            entities.with_entity(world, rider, |rider| finish(world, mob, rider));
            false
        }
        _ => false,
    }
}

fn forget_ride(mob: Entity<'_>) {
    with(|state| state.rides.remove(&mob.id()));
}

/// The ride's single roll: tame and keep the rider, or buck them off and turn on them.
fn finish(world: World<'_>, mob: Entity<'_>, rider: Entity<'_>) {
    let (Some(mobs), Some(ai)) = (MOBS.get(), MOB_AI.get()) else {
        return;
    };
    if roll(RODEO_CHANCE)
        && let Some(owner) = mobs.uuid(rider)
        && taming::tame(world, mob, owner)
    {
        // `setOrderedToSit(false)` and `setCommand(0)`: the new owner keeps riding.
        taming::set_follow(mob);
        taming::feed_particles(world, mob, true);
        return;
    }
    with(|state| state.bucked.insert(mob.id(), tick_count() + BUCK_COOLDOWN));
    rider.stop_riding();
    let (Some(at), Some(from)) = (mob.view(), rider.view()) else {
        return;
    };
    let (dx, dz) = (
        from.position[0] - at.position[0],
        from.position[2] - at.position[2],
    );
    let length = dx.hypot(dz);
    let (dx, dz) = if length > 1.0e-2 {
        (dx / length, dz / length)
    } else {
        let yaw = f64::from(at.yaw).to_radians();
        (yaw.sin(), -yaw.cos())
    };
    rider.change(&EntityChange {
        mask: SET_MOTION,
        motion: [dx, 1.25, dz],
        ..EntityChange::default()
    });
    rider.reset_fall_distance();
    taming::feed_particles(world, mob, false);
    // `triggerFailedTamingAggro` skips creative and spectator riders.
    if !matches!(mobs.game_mode(rider), Some(1 | 3)) {
        ai.set_target(world, mob, Some(rider.id()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varasuchus_food_amounts_and_messages() {
        assert_eq!(
            feeding(false, 180.0, 200.0, false),
            (10.0, "Varasuchus is being healed...")
        );
        assert_eq!(
            feeding(true, 165.0, 200.0, true),
            (35.0, "Feeding Varasuchus...")
        );
        assert_eq!(
            feeding(false, 195.0, 200.0, false),
            (10.0, "Varasuchus is fully healed!")
        );
        assert_eq!(
            feeding(true, 100.0, 200.0, true),
            (35.0, "Varasuchus is being healed...")
        );
    }

    #[test]
    fn rolls_are_percentages() {
        assert!((0..100).all(|_| roll(100.0)));
        assert!((0..100).all(|_| !roll(0.0)));
    }
}
