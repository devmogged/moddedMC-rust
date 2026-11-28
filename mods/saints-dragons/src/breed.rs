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
use serde::{Deserialize, Serialize};

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
    fed: HashMap<i32, u64>,
    courting: HashMap<i32, Courtship>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
const DATA: &str = "saintsdragons:breeding";

/// Animal's InLove, LoveCause and positive Age are remaining loaded-entity ticks.
#[derive(Default, Deserialize, Serialize)]
struct Breeding {
    love: i32,
    cooldown: i32,
    cause: Option<[i32; 4]>,
}

impl Breeding {
    fn load(mob: Entity<'_>) -> Self {
        mob.data(DATA)
            .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, mob: Entity<'_>) {
        if self.love == 0 && self.cooldown == 0 {
            mob.set_data(DATA, &[]);
        } else if let Ok(bytes) = fastnbt::to_bytes(self) {
            mob.set_data(DATA, &bytes);
        }
    }

    fn advance(&mut self) {
        self.love = self.love.saturating_sub(1).max(0);
        self.cooldown = self.cooldown.saturating_sub(1).max(0);
        if self.love == 0 {
            self.cause = None;
        }
    }

    fn cause(&self) -> Option<[u8; 16]> {
        let mut uuid = [0; 16];
        for (chunk, word) in uuid.as_chunks_mut::<4>().0.iter_mut().zip(self.cause?) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        Some(uuid)
    }
}

pub fn tick_age(mob: Entity<'_>) {
    let mut state = Breeding::load(mob);
    if state.love != 0 || state.cooldown != 0 {
        state.advance();
        state.save(mob);
    }
}

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
    let mut breeding = Breeding::load(mob);
    if breeding.cooldown > 0 {
        message(player, "%s is not ready to breed yet", name);
        return Some(UseResult::Success);
    }
    if breeding.love > 0 {
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
    crate::audio::eat(click.world, mob, item);
    with(|state| state.fed.insert(id, now + breeder.feed_ticks()));
    breeding.love = LOVE_TICKS as i32;
    breeding.cause = MOBS.get().and_then(|mobs| mobs.uuid(player)).map(|uuid| {
        std::array::from_fn(|i| i32::from_be_bytes(uuid[i * 4..i * 4 + 4].try_into().unwrap()))
    });
    breeding.save(mob);
    taming::feed_particles(click.world, mob, true);
    message(player, "%s is ready to breed!", name);
    Some(UseResult::Success)
}

/// `breedingAllowed` and the age half of `canMate`.
fn ready(world: World<'_>, mob: Entity<'_>, breeder: &Breeder) -> bool {
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
        && {
            let state = Breeding::load(mob);
            state.love > 0 && state.cooldown == 0
        }
}

fn position(mob: Entity<'_>) -> Option<[f64; 3]> {
    mob.view().map(|view| view.position)
}

fn width(mob: Entity<'_>) -> f64 {
    mob.body().map_or(1.0, |body| body.max[0] - body.min[0])
}

/// `findMate`: the nearest ready dragon of the same species and the other sex that hasn't paired
/// with someone else.
fn find_mate(world: World<'_>, mob: Entity<'_>, breeder: &Breeder) -> Option<i32> {
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
                && ready(world, other, breeder)
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
    if Breeding::load(mob).love == 0 {
        return false;
    }
    with(|state| state.courting.contains_key(&mob.id()))
        || breeder(mob).is_some_and(|breeder| {
            ready(world, mob, breeder) && find_mate(world, mob, breeder).is_some()
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
            if !ready(world, mob, breeder) {
                return false;
            }
            if courtship(mob).is_some() {
                return true;
            }
            let Some(partner) = find_mate(world, mob, breeder) else {
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
                && ready(world, mob, breeder)
                && ENTITIES
                    .get()
                    .and_then(|entities| {
                        entities
                            .with_entity(world, court.partner, |other| ready(world, other, breeder))
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
            lay(world, mob, partner, breeder)
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
fn lay(world: World<'_>, mob: Entity<'_>, partner: Entity<'_>, breeder: &Breeder) {
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
        let causes = [mob, partner].map(|dragon| Breeding::load(dragon).cause());
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
    for dragon in [mob, partner] {
        Breeding {
            cooldown: COOLDOWN_TICKS as i32,
            ..Breeding::default()
        }
        .save(dragon);
        with(|state| release(state, dragon.id()));
    }
    if let Some(ai) = MOB_AI.get() {
        ai.stop(mob);
        ai.stop(partner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breeding_countdowns_and_cause_survive_nbt_roundtrip() {
        let mut state = Breeding {
            love: 2,
            cooldown: 4,
            cause: Some([-1; 4]),
        };
        state.advance();
        let bytes = fastnbt::to_bytes(&state).unwrap();
        let mut restored: Breeding = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(
            (restored.love, restored.cooldown, restored.cause()),
            (1, 3, Some([255; 16]))
        );
        restored.advance();
        assert_eq!(
            (restored.love, restored.cooldown, restored.cause()),
            (0, 2, None)
        );
        let restored: Breeding =
            fastnbt::from_bytes(&fastnbt::to_bytes(&restored).unwrap()).unwrap();
        assert_eq!((restored.cooldown, restored.cause()), (2, None));
    }

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
