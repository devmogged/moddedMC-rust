//! 0.9.88 breeding: crouch-feeding a tame adult its food puts it in love, and two tame adults of
//! one species and opposite sexes in love court for a few seconds and lay that species' egg.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Pos,
    content::World,
    entities::{Entity, EntityChange, SET_MOTION},
    interaction::{UseMob, UseResult},
    mob_ai::goal_phase,
    tick_count,
};

use crate::{ENTITIES, MOB_AI, MOBS, TAGS, ai::random, appearance::Appearance, taming};

pub const GOAL: u64 = 13;
/// Vanilla `Animal.setInLove`.
const LOVE_TICKS: u64 = 600;
/// `setAge(6000)` after laying.
const COOLDOWN_TICKS: u64 = 6000;
const FEED_COOLDOWN: u64 = 61;
const MAX_ATTEMPT_TICKS: u64 = 300;

/// A species' `BREED_PARTNER_RANGE` and `BREED_DISTANCE_SQR`.
struct Breeder {
    name: &'static str,
    range: f64,
    distance_sqr: f64,
}

const BREEDERS: [Breeder; 8] = [
    Breeder {
        name: "atroxiia",
        range: 20.0,
        distance_sqr: 36.0,
    },
    Breeder {
        name: "cindervane",
        range: 20.0,
        distance_sqr: 2500.0,
    },
    Breeder {
        name: "ignivorus",
        range: 20.0,
        distance_sqr: 2500.0,
    },
    Breeder {
        name: "raevyx",
        range: 8.0,
        distance_sqr: 16.0,
    },
    Breeder {
        name: "stegonaut",
        range: 20.0,
        distance_sqr: 2500.0,
    },
    Breeder {
        name: "varasuchus",
        range: 30.0,
        distance_sqr: 16.0,
    },
    Breeder {
        name: "volitans",
        range: 20.0,
        distance_sqr: 16.0,
    },
    Breeder {
        name: "nulljaw",
        range: 24.0,
        distance_sqr: 9.0,
    },
];

impl Breeder {
    fn courtship_ticks(&self) -> u64 {
        match self.name {
            "volitans" | "nulljaw" => 60,
            _ => 60 + random(50),
        }
    }

    fn feed_ticks(&self) -> u64 {
        if self.name == "nulljaw" {
            24
        } else {
            FEED_COOLDOWN
        }
    }
}

#[derive(Clone, Copy)]
struct Courtship {
    partner: i32,
    /// When the coordinator lays the egg; never for the other dragon.
    egg_at: u64,
    ends: u64,
}

#[derive(Default)]
struct State {
    love: HashMap<i32, u64>,
    love_cause: HashMap<i32, [u8; 16]>,
    cooldown: HashMap<i32, u64>,
    fed: HashMap<i32, u64>,
    courting: HashMap<i32, Courtship>,
}

// shortcut: love and the post-laying cooldown live in memory, so a restart clears them.
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default())
}

fn breeder(mob: Entity<'_>) -> Option<&'static Breeder> {
    let name = MOBS.get()?.kind(mob)?.strip_prefix("saintsdragons:")?;
    BREEDERS.iter().find(|breeder| breeder.name == name)
}

pub fn breeds(species: &str) -> bool {
    BREEDERS.iter().any(|breeder| breeder.name == species)
}

pub fn forget(id: i32) {
    with(|state| {
        state.love.remove(&id);
        state.love_cause.remove(&id);
        state.cooldown.remove(&id);
        state.fed.remove(&id);
        release(state, id);
    });
}

fn release(state: &mut State, id: i32) {
    if let Some(court) = state.courting.remove(&id)
        && state
            .courting
            .get(&court.partner)
            .is_some_and(|other| other.partner == id)
    {
        state.courting.remove(&court.partner);
    }
}

fn in_love(state: &State, id: i32, now: u64) -> bool {
    state.love.get(&id).is_some_and(|until| *until > now)
}

fn message(player: Entity<'_>, template: &str, name: &str) {
    if let Some(mobs) = MOBS.get() {
        mobs.message(player, &template.replacen("%s", name, 1), true);
    }
}

/// `DragonBreedingInteractionHelper.handleBreeding`, for a crouching player with the species'
/// food. None leaves the click to the other handlers.
pub fn feed(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    let (player, mob) = (click.player?, click.mob?);
    let breeder = breeder(mob)?;
    let foods = format!("saintsdragons:foods/{}", breeder.name);
    if !click.sneaking || !taming::is_tamed(mob) || !TAGS.get()?.contains("item", &foods, item) {
        return None;
    }
    if breeder.name == "varasuchus" && crate::MOBS.get()?.game_mode(player) == Some(3) {
        return None;
    }
    let name = crate::roster::display(breeder.name);
    let now = tick_count();
    let id = mob.id();
    let busy = with(|state| {
        state.fed.retain(|_, until| *until > now);
        state.fed.contains_key(&id)
    });
    if busy || (breeder.name == "varasuchus" && !crate::baby::start_eating(id, 0)) {
        message(player, "%s is still eating...", name);
        return Some(UseResult::Consume);
    }
    if crate::eggs::is_juvenile(mob) {
        message(player, "%s is too young to breed", name);
        return Some(UseResult::Success);
    }
    if with(|state| state.cooldown.get(&id).is_some_and(|until| *until > now)) {
        message(player, "%s is not ready to breed yet", name);
        return Some(UseResult::Success);
    }
    if with(|state| in_love(state, id, now)) {
        message(player, "%s is already ready to breed", name);
        return Some(UseResult::Success);
    }
    if breeder.name == "varasuchus" && !crate::baby::start_eating(id, FEED_COOLDOWN) {
        message(player, "%s is still eating...", name);
        return Some(UseResult::Consume);
    }
    if !click.creative {
        click.stack.shrink(1);
    }
    crate::audio::eat(click.world, mob);
    with(|state| {
        state.fed.insert(id, now + breeder.feed_ticks());
        state.love.insert(id, now + LOVE_TICKS);
        if let Some(uuid) = MOBS.get().and_then(|mobs| mobs.uuid(player)) {
            state.love_cause.insert(id, uuid);
        }
    });
    taming::feed_particles(click.world, mob, true);
    message(player, "%s is ready to breed!", name);
    Some(UseResult::Success)
}

/// `breedingAllowed` and the age half of `canMate`.
fn ready(world: World<'_>, mob: Entity<'_>, breeder: &Breeder, now: u64) -> bool {
    taming::is_tamed(mob)
        && !crate::eggs::is_juvenile(mob)
        && (breeder.name != "volitans"
            || (position(mob).is_some_and(|at| crate::eggs::water_at(world, block(at)))
                && MOBS
                    .get()
                    .is_some_and(|mobs| mobs.passengers(mob).is_empty())))
        && MOBS
            .get()
            .and_then(|mobs| mobs.health(mob))
            .is_some_and(|(health, _)| health > 0.0)
        && with(|state| {
            in_love(state, mob.id(), now)
                && !state
                    .cooldown
                    .get(&mob.id())
                    .is_some_and(|until| *until > now)
        })
}

fn position(mob: Entity<'_>) -> Option<[f64; 3]> {
    mob.view().map(|view| view.position)
}

fn width(mob: Entity<'_>) -> f64 {
    mob.body().map_or(1.0, |body| body.max[0] - body.min[0])
}

/// `findMate`: the nearest ready dragon of the same species and the other sex that hasn't paired
/// with someone else.
fn find_mate(world: World<'_>, mob: Entity<'_>, breeder: &Breeder, now: u64) -> Option<i32> {
    let (mobs, entities) = (MOBS.get()?, ENTITIES.get()?);
    let kind = mobs.kind(mob)?;
    let at = position(mob)?;
    let female = Appearance::is_female(mob);
    let reach = breeder.range + width(mob) / 2.0;
    let mut best: Option<(f64, i32)> = None;
    for id in entities.entities_in(world, at.map(|c| c - reach), at.map(|c| c + reach), -1) {
        if id == mob.id() {
            continue;
        }
        let candidate = entities.with_entity(world, id, |other| {
            let fits = mobs.kind(other) == Some(kind)
                && Appearance::is_female(other) != female
                && ready(world, other, breeder, now)
                && with(|state| {
                    state
                        .courting
                        .get(&id)
                        .is_none_or(|court| court.partner == mob.id())
                });
            fits.then(|| position(other)).flatten()
        });
        if let Some(Some(other)) = candidate {
            let distance = distance_sqr(at, other);
            if best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, id));
            }
        }
    }
    best.map(|(_, id)| id)
}

fn distance_sqr(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| (a - b).powi(2)).sum()
}

/// Whether a ready dragon has a mate to court, so it gets up from sitting.
pub fn wants_mate(world: World<'_>, mob: Entity<'_>) -> bool {
    let now = tick_count();
    if !with(|state| in_love(state, mob.id(), now)) {
        return false;
    }
    with(|state| state.courting.contains_key(&mob.id()))
        || breeder(mob).is_some_and(|breeder| {
            ready(world, mob, breeder, now) && find_mate(world, mob, breeder, now).is_some()
        })
}

fn courtship(mob: Entity<'_>) -> Option<Courtship> {
    with(|state| {
        let court = *state.courting.get(&mob.id())?;
        (state.courting.get(&court.partner)?.partner == mob.id()).then_some(court)
    })
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let now = tick_count();
    let Some(breeder) = breeder(mob) else {
        return false;
    };
    match phase {
        goal_phase::CAN_START => {
            if !ready(world, mob, breeder, now) {
                return false;
            }
            if courtship(mob).is_some() {
                return true;
            }
            let Some(partner) = find_mate(world, mob, breeder, now) else {
                return false;
            };
            // `start`: the dragon with the lower id lays the egg after 60 to 109 ticks.
            let egg_at = now + breeder.courtship_ticks();
            let ends = now + MAX_ATTEMPT_TICKS;
            let (first, second) = if mob.id() < partner {
                (egg_at, u64::MAX)
            } else {
                (u64::MAX, egg_at)
            };
            with(|state| {
                release(state, mob.id());
                release(state, partner);
                state.courting.insert(
                    mob.id(),
                    Courtship {
                        partner,
                        egg_at: first,
                        ends,
                    },
                );
                state.courting.insert(
                    partner,
                    Courtship {
                        partner: mob.id(),
                        egg_at: second,
                        ends,
                    },
                );
            });
            true
        }
        goal_phase::CONTINUE => {
            let Some(court) = courtship(mob) else {
                return false;
            };
            now <= court.ends
                && ready(world, mob, breeder, now)
                && ENTITIES
                    .get()
                    .and_then(|entities| {
                        entities.with_entity(world, court.partner, |other| {
                            ready(world, other, breeder, now)
                        })
                    })
                    .unwrap_or(false)
        }
        goal_phase::TICK => {
            tick(world, mob, breeder, now);
            false
        }
        goal_phase::STOP => {
            with(|state| release(state, mob.id()));
            if let Some(ai) = MOB_AI.get() {
                ai.stop(mob);
            }
            false
        }
        _ => false,
    }
}

fn tick(world: World<'_>, mob: Entity<'_>, breeder: &Breeder, now: u64) {
    let (Some(court), Some(entities), Some(ai), Some(at)) =
        (courtship(mob), ENTITIES.get(), MOB_AI.get(), position(mob))
    else {
        return;
    };
    let Some(Some((other, other_width))) = entities.with_entity(world, court.partner, |other| {
        position(other).map(|position| (position, width(other)))
    }) else {
        return;
    };
    // `closeEnough`: centers within the breed distance, or boxes within two blocks.
    let horizontal = (at[0] - other[0]).hypot(at[2] - other[2]);
    let close = distance_sqr(at, other) <= breeder.distance_sqr.min(16.0)
        || (horizontal <= (width(mob) + other_width) / 2.0 + 2.0
            && (at[1] - other[1]).abs() <= 2.5);
    ai.look_at(mob, other);
    if close {
        ai.stop(mob);
        if matches!(breeder.name, "volitans" | "nulljaw") {
            mob.change(&EntityChange {
                mask: SET_MOTION,
                motion: [0.0; 3],
                ..EntityChange::default()
            });
        }
    } else if matches!(breeder.name, "volitans" | "nulljaw") {
        // shortcut: direct collision-checked motion until the shared dragon waypoint planner is ported.
        let speed = if breeder.name == "nulljaw" {
            0.22
        } else {
            1.42 * 0.20
        };
        let delta = crate::flight::offset(other, at);
        let motion =
            delta.map(|v| v * speed / crate::flight::length_squared(delta).sqrt().max(speed));
        if mob
            .body()
            .is_some_and(|body| crate::flight::clear_body(world, body, motion))
        {
            mob.reset_fall_distance();
            mob.change(&EntityChange {
                mask: SET_MOTION,
                motion,
                ..EntityChange::default()
            });
        }
    } else if (now + mob.id() as u64).is_multiple_of(10) {
        ai.walk_to(mob, other, 1.0);
    }
    if close && now >= court.egg_at {
        entities.with_entity(world, court.partner, |partner| {
            lay(world, mob, partner, breeder, now)
        });
    }
}

fn block(at: [f64; 3]) -> Pos {
    Pos {
        x: at[0].floor() as i32,
        y: at[1].floor() as i32,
        z: at[2].floor() as i32,
    }
}

/// `breed`: the egg goes between the pair, or near the mother.
fn lay(world: World<'_>, mob: Entity<'_>, partner: Entity<'_>, breeder: &Breeder, now: u64) {
    let (Some(at), Some(other)) = (position(mob), position(partner)) else {
        return;
    };
    let mother = if Appearance::is_female(mob) {
        mob
    } else {
        partner
    };
    let Some(mother_at) = position(mother) else {
        return;
    };
    let midpoint = [
        (at[0] + other[0]) * 0.5,
        if matches!(breeder.name, "volitans" | "nulljaw") {
            (at[1] + other[1]) * 0.5
        } else {
            at[1].min(other[1])
        },
        (at[2] + other[2]) * 0.5,
    ];
    let born = if breeder.name == "nulljaw" {
        crate::eggs::birth_nulljaw(
            world,
            midpoint,
            taming::owner(mob).or_else(|| taming::owner(partner)),
        )
    } else {
        crate::eggs::lay(
            world,
            [block(midpoint), block(mother_at)],
            breeder.name,
            taming::owner(mother),
        )
    };
    if !born {
        return;
    }
    taming::feed_particles(world, mother, true);
    if let Some(mobs) = MOBS.get() {
        if mobs.boolean_gamerule(world, "minecraft:mob_drops") == Some(true) {
            let at = if breeder.name == "nulljaw" {
                midpoint
            } else {
                mother_at
            };
            let _ = mobs.award_experience(world, at, (1 + random(7)) as u32);
        }
        let causes =
            with(|state| [mob.id(), partner.id()].map(|id| state.love_cause.get(&id).copied()));
        if let Some(entities) = ENTITIES.get() {
            for uuid in causes.into_iter().flatten() {
                if let Some(id) = mobs.by_uuid(world, &uuid)
                    && entities.with_entity(world, id, |player| {
                        mobs.award_stat(player, "minecraft:custom", "minecraft:animals_bred", 1)
                    }) == Some(Some(true))
                {
                    break;
                }
            }
        }
    }
    // shortcut: the contextual BRED_ANIMALS advancement trigger still needs a host call.
    with(|state| {
        for id in [mob.id(), partner.id()] {
            state.love.remove(&id);
            state.love_cause.remove(&id);
            state.cooldown.insert(id, now + COOLDOWN_TICKS);
            release(state, id);
        }
    });
    if let Some(ai) = MOB_AI.get() {
        ai.stop(mob);
        ai.stop(partner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakup_releases_both_dragons() {
        let mut state = State::default();
        let court = |partner| Courtship {
            partner,
            egg_at: 0,
            ends: 0,
        };
        state.courting.insert(1, court(2));
        state.courting.insert(2, court(1));
        state.courting.insert(3, court(2));
        release(&mut state, 1);
        assert!(!state.courting.contains_key(&2));
        // A one-sided claim stays until its own dragon lets go.
        assert!(state.courting.contains_key(&3));
        assert!(breeds("raevyx") && breeds("volitans") && breeds("nulljaw"));
    }

    #[test]
    fn aquatic_and_live_birth_courtship_use_fixed_released_timings() {
        for (name, range, distance, feed) in
            [("volitans", 20.0, 16.0, 61), ("nulljaw", 24.0, 9.0, 24)]
        {
            let b = BREEDERS.iter().find(|b| b.name == name).unwrap();
            assert_eq!(
                (b.range, b.distance_sqr, b.feed_ticks(), b.courtship_ticks()),
                (range, distance, feed, 60)
            );
        }
    }
}
