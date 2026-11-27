//! 0.9.88 `DragonTamingStunComponent`: Atroxiia, Ignivorus, Raevyx and Volitans are tamed by
//! weakening them to their stun health and feeding them while they are held down.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::Entity,
    interaction::{UseMob, UseResult},
    mob_ai::goal_phase,
    tick_count,
};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOB_HEALTH, MOBS, TAGS, ai::random, taming};

pub const GOAL: u64 = 12;
const HOLD_TICKS: u64 = 20 * 30;

/// One species' `dragon_attributes` taming values, feeding cooldowns and messages.
struct Profile {
    kind: &'static str,
    display: &'static str,
    foods: &'static str,
    stun_health: f32,
    base: f64,
    hearty: f64,
    /// Food-specific taming chances in percent.
    chances: &'static [(&'static str, f64)],
    /// Untamed and tamed `setFeedingCooldown` ticks.
    cooldowns: (u64, u64),
    /// Tamed heal for ordinary food, hearty meals and raw beef.
    heals: (f32, f32, f32),
    still_eating: &'static str,
    dazed: &'static str,
    weakened: &'static str,
    failed: &'static str,
    timeout: &'static str,
}

const PROFILES: [Profile; 4] = [
    Profile {
        kind: "saintsdragons:raevyx",
        display: "Raevyx",
        foods: "saintsdragons:foods/raevyx",
        stun_health: 60.0,
        base: 20.0,
        hearty: 33.3333,
        chances: &[("minecraft:mutton", 20.0), ("minecraft:porkchop", 20.0)],
        cooldowns: (61, 22),
        heals: (10.0, 28.0, 10.0),
        still_eating: "%s is still eating...",
        dazed: "%s is still dazed from the fight...",
        weakened: "%s is too strong! Weaken it to %s HP first.",
        failed: "%s rejected you, but seems willing to try again...",
        timeout: "You took too long to decide. %s has recovered and resumed combat.",
    },
    Profile {
        kind: "saintsdragons:ignivorus",
        display: "Ignivorus",
        foods: "saintsdragons:foods/ignivorus",
        stun_health: 100.0,
        base: 14.2857,
        hearty: 25.0,
        chances: &[
            ("minecraft:beef", 20.0),
            ("minecraft:mutton", 14.2857),
            ("minecraft:porkchop", 14.2857),
        ],
        cooldowns: (20, 23),
        heals: (10.0, 30.0, 16.0),
        still_eating: "%s is still chewing!",
        dazed: "%s is exhausted from battle...",
        weakened: "%s burns too hot! Weaken it to %s HP first.",
        failed: "%s's pride won't let it submit... yet.",
        timeout: "You hesitated too long. %s reignites and returns to battle!",
    },
    Profile {
        kind: "saintsdragons:volitans",
        display: "Volitans",
        foods: "saintsdragons:foods/volitans",
        stun_health: 60.0,
        base: 20.0,
        hearty: 30.0,
        chances: &[],
        cooldowns: (61, 24),
        heals: (10.0, 28.0, 10.0),
        still_eating: "%s is still eating...",
        dazed: "%s is stunned but not ready to be fed yet.",
        weakened: "%s must be weakened to %s HP before it can be fed.",
        failed: "%s resists your attempt and regains its will to fight.",
        timeout: "%s recovers and slips back into a hostile state.",
    },
    Profile {
        kind: "saintsdragons:atroxiia",
        display: "Atroxiia",
        foods: "saintsdragons:foods/atroxiia",
        stun_health: 60.0,
        base: 20.0,
        hearty: 33.3333,
        chances: &[],
        cooldowns: (40, 40),
        heals: (10.0, 28.0, 10.0),
        still_eating: "%s is still eating...",
        dazed: "%s is still reeling from the battle...",
        weakened: "%s remains defiant! Weaken it to %s HP first.",
        failed: "%s rejects your offer and gathers its strength again.",
        timeout: "%s recovers from the fight and rises again.",
    },
];

#[derive(Clone, Copy)]
struct Stun {
    /// Held down and waiting for food, until `until`; otherwise recovering after a feed.
    awaiting: bool,
    until: u64,
    /// Health a failed feed restores, or 0.
    target: f32,
    last: u64,
}

#[derive(Default)]
struct State {
    stunned: HashMap<i32, Stun>,
    fed: HashMap<i32, u64>,
}

// shortcut: the hold lives in memory, so a restart drops it; a dragon still at or below its
// stun health is held again on its next AI tick, like `tickRecovery`.
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default())
}

fn profile(kind: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|profile| profile.kind == kind)
}

pub fn uses_stun(species: &str) -> bool {
    profile(&format!("saintsdragons:{species}")).is_some()
}

fn text(template: &str, name: &str) -> String {
    template.replacen("%s", name, 1)
}

/// The species profile and health of an untamed adult that can be stunned.
fn wild(mob: Entity<'_>) -> Option<(&'static Profile, f32, f32)> {
    let mobs = MOBS.get()?;
    let profile = profile(mobs.kind(mob)?)?;
    let (health, max) = mobs.health(mob)?;
    (health > 0.0 && !taming::is_tamed(mob) && !crate::eggs::is_juvenile(mob))
        .then_some((profile, health, max))
}

fn threshold(profile: &Profile, max: f32) -> f32 {
    profile.stun_health.clamp(0.0, max)
}

pub fn forget(id: i32) {
    with(|state| {
        state.stunned.remove(&id);
        state.fed.remove(&id);
    });
}

fn clear_stun(id: i32) {
    // Java clears taming recovery independently of the feeding cooldown.
    with(|state| state.stunned.remove(&id));
}

/// `tryEnterHoldStateFromDamage`: a hit that would take the dragon past its stun health leaves
/// it at that health and holds it instead. False cancels the hit.
pub fn hurt(world: World<'_>, mob: Entity<'_>, damage_type: &str, amount: f32) -> bool {
    // `DamageTypeTags.BYPASSES_INVULNERABILITY`.
    if amount <= 0.0
        || matches!(
            damage_type,
            "minecraft:out_of_world" | "minecraft:generic_kill"
        )
    {
        return true;
    }
    let Some((profile, health, max)) = wild(mob) else {
        return true;
    };
    let threshold = threshold(profile, max);
    if threshold <= 0.0
        || health <= threshold
        || health - amount > threshold
        || with(|state| state.stunned.contains_key(&mob.id()))
    {
        return true;
    }
    if let Some(vitals) = MOB_HEALTH.get() {
        vitals.set(mob, threshold.max(1.0));
    }
    hold(world, mob);
    false
}

/// `enterHoldState`.
fn hold(world: World<'_>, mob: Entity<'_>) {
    let now = tick_count();
    with(|state| {
        state.stunned.insert(
            mob.id(),
            Stun {
                awaiting: true,
                until: now + HOLD_TICKS,
                target: 0.0,
                last: now,
            },
        )
    });
    lock(world, mob);
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        effects.play_sound(
            world,
            view.position,
            "minecraft:entity.arrow.hit_player",
            SOUND_NEUTRAL,
            1.5,
            0.8,
        );
    }
}

/// `ensureStunState`: no target, no path, no riders, and gravity back on.
fn lock(world: World<'_>, mob: Entity<'_>) {
    let (Some(mobs), Some(ai), Some(entities)) = (MOBS.get(), MOB_AI.get(), ENTITIES.get()) else {
        return;
    };
    ai.stop(mob);
    ai.set_target(world, mob, None);
    mobs.set_no_gravity(mob, false);
    for rider in mobs.passengers(mob) {
        entities.with_entity(world, rider, |rider| rider.stop_riding());
    }
}

/// `tickRecovery`, run from the hold goal: true while the dragon stays held.
fn update(world: World<'_>, mob: Entity<'_>) -> bool {
    let Some((profile, health, max)) = wild(mob) else {
        clear_stun(mob.id());
        return false;
    };
    let now = tick_count();
    let Some(stun) = with(|state| state.stunned.get(&mob.id()).copied()) else {
        if health <= threshold(profile, max) {
            hold(world, mob);
            return true;
        }
        return false;
    };
    if stun.awaiting {
        if now < stun.until {
            return true;
        }
        if let Some(vitals) = MOB_HEALTH.get() {
            vitals.set(mob, max);
        }
        notify(world, mob, &text(profile.timeout, profile.display));
        clear_stun(mob.id());
        return false;
    }
    if stun.target > 0.0 && health + 0.5 < stun.target {
        // The goal may not tick every game tick, so heal for the ticks since the last one.
        let ticks = now.saturating_sub(stun.last).max(1) as f32;
        let step = (max * 0.02).max(2.0) * ticks;
        if let Some(mobs) = MOBS.get() {
            mobs.heal(mob, step.min(stun.target - health));
        }
        with(|state| {
            if let Some(stun) = state.stunned.get_mut(&mob.id()) {
                stun.last = now;
            }
        });
        return true;
    }
    if now < stun.until {
        return true;
    }
    clear_stun(mob.id());
    false
}

/// The timeout message goes to players within 32 blocks.
fn notify(world: World<'_>, mob: Entity<'_>, message: &str) {
    let (Some(mobs), Some(entities), Some(at)) = (
        MOBS.get(),
        ENTITIES.get(),
        mob.view().map(|view| view.position),
    ) else {
        return;
    };
    for id in entities.players(world) {
        entities.with_entity(world, id, |player| {
            if player
                .view()
                .is_some_and(|view| (0..3).all(|i| (view.position[i] - at[i]).abs() <= 34.0))
            {
                mobs.message(player, message, true);
            }
        });
    }
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => update(world, mob),
        goal_phase::START | goal_phase::TICK => {
            lock(world, mob);
            false
        }
        _ => false,
    }
}

/// A species' taming chance in percent for `item` and its untamed feeding cooldown.
pub fn taming_chance(kind: &str, item: &str) -> Option<(f64, u64)> {
    profile(kind).map(|profile| (chance(profile, item), profile.cooldowns.0))
}

fn chance(profile: &Profile, item: &str) -> f64 {
    if item == taming::HEARTY_MEAL {
        return profile.hearty;
    }
    profile
        .chances
        .iter()
        .find(|(food, _)| *food == item)
        .map_or(profile.base, |(_, chance)| *chance)
}

/// The interaction handlers' food branch for the stun species. None leaves the click to the
/// shared handling.
pub fn feed(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    let (player, mob, mobs) = (click.player?, click.mob?, MOBS.get()?);
    let profile = profile(mobs.kind(mob)?)?;
    if !TAGS.get()?.contains("item", profile.foods, item) {
        return None;
    }
    // shortcut: baby food taming and growth are a separate mechanic; babies ignore food here.
    if crate::eggs::is_juvenile(mob) {
        return None;
    }
    let (health, max) = mobs.health(mob)?;
    let now = tick_count();
    let tamed = taming::is_tamed(mob);
    let stun = with(|state| state.stunned.get(&mob.id()).copied());
    if !tamed && stun.is_some_and(|stun| !stun.awaiting) {
        mobs.message(player, &text(profile.dazed, profile.display), true);
        return Some(UseResult::Consume);
    }
    let busy = with(|state| {
        state.fed.retain(|_, until| *until > now);
        state.fed.contains_key(&mob.id())
    });
    if busy {
        mobs.message(player, &text(profile.still_eating, profile.display), true);
        return Some(UseResult::Consume);
    }
    let threshold = threshold(profile, max);
    if !tamed && stun.is_none() && health > threshold + 1.0 {
        let message = text(profile.weakened, profile.display).replacen(
            "%s",
            &format!("{}", threshold.round()),
            1,
        );
        mobs.message(player, &message, true);
        return Some(UseResult::Consume);
    }
    let cooldown = if tamed {
        profile.cooldowns.1
    } else {
        profile.cooldowns.0
    };
    with(|state| state.fed.insert(mob.id(), now + cooldown));
    if !click.creative {
        click.stack.shrink(1);
    }
    crate::audio::eat(click.world, mob);
    let hearty = item == taming::HEARTY_MEAL;
    let hungry = crate::hunger::feed(mob, hearty);
    if tamed {
        if hearty {
            mob.add_effect("minecraft:regeneration", 200, 1, false, true);
        }
        let (normal, hearty_heal, beef) = profile.heals;
        let heal = match item {
            taming::HEARTY_MEAL => hearty_heal,
            "minecraft:beef" => beef,
            _ => normal,
        };
        mobs.heal(mob, heal);
        taming::feed_particles(click.world, mob, true);
        let message = if health + heal >= max {
            if hungry {
                "Feeding %s..."
            } else {
                "%s is fully healed!"
            }
        } else {
            "%s is being healed..."
        };
        mobs.message(player, &text(message, profile.display), true);
        return Some(UseResult::Success);
    }
    // `enterStun`: the feed ends the hold and starts a short grace period.
    with(|state| {
        state.stunned.insert(
            mob.id(),
            Stun {
                awaiting: false,
                until: now + 20,
                target: 0.0,
                last: now,
            },
        )
    });
    let owner = mobs.uuid(player)?;
    let roll = random(1_000_000) as f64 / 10_000.0;
    if roll < chance(profile, item) && taming::tame(click.world, mob, owner) {
        clear_stun(mob.id());
        if hearty {
            mob.add_effect("minecraft:regeneration", 200, 1, false, true);
        }
        taming::feed_particles(click.world, mob, true);
    } else {
        // `setRecoveryTarget(getMaxHealth())`.
        with(|state| {
            state.stunned.insert(
                mob.id(),
                Stun {
                    awaiting: false,
                    until: now + 40,
                    target: max,
                    last: now,
                },
            )
        });
        taming::feed_particles(click.world, mob, false);
        mobs.message(player, &text(profile.failed, profile.display), true);
    }
    Some(UseResult::Success)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_match_the_released_attributes() {
        let raevyx = profile("saintsdragons:raevyx").unwrap();
        assert_eq!(chance(raevyx, "minecraft:mutton"), 20.0);
        assert_eq!(chance(raevyx, "minecraft:cod"), 20.0);
        assert_eq!(chance(raevyx, taming::HEARTY_MEAL), 33.3333);
        let ignivorus = profile("saintsdragons:ignivorus").unwrap();
        assert_eq!(chance(ignivorus, "minecraft:beef"), 20.0);
        assert_eq!(threshold(ignivorus, 450.0), 100.0);
        assert_eq!(threshold(ignivorus, 50.0), 50.0);
        assert!(uses_stun("volitans") && uses_stun("atroxiia") && !uses_stun("cindervane"));
        assert_eq!(
            text(raevyx.weakened, "Raevyx").replacen("%s", "60", 1),
            "Raevyx is too strong! Weaken it to 60 HP first."
        );
    }
}
