//! 0.9.88 taming for the two species without a stun: Stegonaut takes food, and Varasuchus is
//! ridden until it either gives in or bucks the rider off.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{ENTITY_ALIVE, ENTITY_PLAYER, Entity, EntityChange, SET_MOTION},
    interaction::{UseMob, UseResult},
    mob_ai::goal_phase,
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

// Java does not write these fields to NBT. A loaded mounted wild ride starts again.
static RODEOS: LazyLock<Mutex<HashMap<i32, Rodeo>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Default)]
struct Rodeo {
    /// Remaining ticks of the current mounted attempt.
    remaining: i32,
    cooldown: i32,
}

impl Rodeo {
    fn load(mob: Entity<'_>) -> Self {
        RODEOS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&mob.id())
            .copied()
            .unwrap_or_default()
    }

    fn save(&self, mob: Entity<'_>) {
        let mut states = RODEOS.lock().unwrap_or_else(PoisonError::into_inner);
        if self.remaining == 0 && self.cooldown == 0 {
            states.remove(&mob.id());
        } else {
            states.insert(mob.id(), *self);
        }
    }

    fn clear_ride(&mut self) {
        self.remaining = 0;
    }
}

pub fn forget(id: i32) {
    RODEOS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
}

fn mounted_rider(world: World<'_>, mob: Entity<'_>) -> Option<i32> {
    let id = MOBS.get()?.passengers(mob).first().copied()?;
    ENTITIES.get()?.with_entity(world, id, |rider| {
        let flags = rider.flags();
        (rider
            .view()
            .is_some_and(|view| view.kind & ENTITY_PLAYER != 0)
            && flags & ENTITY_ALIVE != 0)
            .then_some(id)
    })?
}

/// Wild ride counters advance only while the entity is loaded.
pub fn tick_state(world: World<'_>, mob: Entity<'_>) {
    let mut state = Rodeo::load(mob);
    if !wild_adult(mob) {
        if state.remaining > 0 || state.cooldown > 0 {
            Rodeo::default().save(mob);
        }
        return;
    }
    if state.cooldown > 0 {
        state.cooldown -= 1;
        state.save(mob);
    }
    let rider = mounted_rider(world, mob);
    if rider.is_some() {
        if state.remaining <= 0 {
            // Java also restarts an untamed ride if a loaded mount already has a rider.
            state.remaining = RIDE_TICKS as i32;
            state.save(mob);
        }
    } else if state.remaining > 0 {
        state.clear_ride();
        state.save(mob);
    }
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
    crate::audio::eat(click.world, mob, item);
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
        crate::audio::eat(click.world, mob, item);
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
    let mut state = Rodeo::load(mob);
    if state.cooldown > 0 {
        let seconds = (state.cooldown + 19) / 20;
        mobs.message(
            player,
            &format!("Nuh-uh. Varasuchus said no. Try again in {seconds} seconds."),
            true,
        );
        return Some(UseResult::Fail);
    }
    taming::configure_seats(mob, kind, false);
    if !mobs.passengers(mob).is_empty() || !player.start_riding(mob.id(), false) {
        return Some(UseResult::Fail);
    }
    state.remaining = RIDE_TICKS as i32;
    state.save(mob);
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
    let mut state = Rodeo::load(mob);
    if state.remaining <= 0 {
        return false;
    }
    let (Some(ai), Some(entities)) = (MOB_AI.get(), ENTITIES.get()) else {
        return false;
    };
    let rider = mounted_rider(world, mob);
    if !wild_adult(mob) || rider.is_none() {
        state.clear_ride();
        state.save(mob);
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
            mob.change(&EntityChange {
                mask: SET_MOTION,
                motion: [0.0; 3],
                ..EntityChange::default()
            });
            if phase == goal_phase::START {
                return false;
            }
            state.remaining -= 1;
            if state.remaining > 0 {
                state.save(mob);
                entities.with_entity(world, rider, |rider| rider.reset_fall_distance());
                return false;
            }
            state.clear_ride();
            state.save(mob);
            entities.with_entity(world, rider, |rider| finish(world, mob, rider));
            false
        }
        _ => false,
    }
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
    Rodeo {
        cooldown: BUCK_COOLDOWN as i32,
        ..Rodeo::default()
    }
    .save(mob);
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
