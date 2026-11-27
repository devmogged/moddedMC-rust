use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{content::World, entities::Entity, mob_ai::goal_phase, tick_count};
use serde::{Deserialize, Serialize};

use crate::{ENTITIES, MOB_AI, MOBS, flight, taming};

pub const GOAL: u64 = 11;
const DATA: &str = "saintsdragons:roost";
const ANCHOR_TAG: &str = "saintsdragons.sleep_anchor";
const CHECK_INTERVAL: u64 = 19;
const SETTLE_TICKS: u64 = 60;

static ACTIVE: Mutex<Option<HashMap<i32, Return>>> = Mutex::new(None);
static SUPPRESSED: Mutex<Option<HashMap<i32, u64>>> = Mutex::new(None);

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Roost {
    home: [i32; 3],
    anchor: Option<[i32; 3]>,
}

impl Roost {
    fn save(self, mob: Entity<'_>) {
        if let Ok(nbt) = fastnbt::to_bytes(&self) {
            mob.set_data(DATA, &nbt);
        }
    }

    fn resolve_anchor(&mut self, world: World<'_>, mob: Entity<'_>) {
        let Some(entities) = ENTITIES.get() else {
            return;
        };
        let center = self.home.map(|coordinate| f64::from(coordinate) + 0.5);
        // shortcut: both released anchors are within two blocks of their resident's spawn.
        // Custom templates can fall back to the spawn block if they move the anchor farther away.
        let nearest = entities
            .entities_in(
                world,
                center.map(|coordinate| coordinate - 2.0),
                center.map(|coordinate| coordinate + 2.0),
                mob.id(),
            )
            .into_iter()
            .filter_map(|id| {
                entities.with_entity(world, id, |entity| {
                    if entity.type_id() != "marker" || !entity.has_scoreboard_tag(ANCHOR_TAG) {
                        return None;
                    }
                    let position = entity.view()?.position;
                    let distance = flight::length_squared(flight::offset(position, center));
                    Some((distance, id, block(position)))
                })?
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        if let Some((_, _, anchor)) = nearest {
            self.anchor = Some(anchor);
            self.save(mob);
        }
    }
}

#[derive(Clone, Copy)]
struct Profile {
    territory: f64,
    return_radius: f64,
    sleep_radius: f64,
    fall_asleep: u64,
    wake_up: u64,
}

impl Profile {
    fn for_mob(mob: Entity<'_>) -> Option<Self> {
        Some(match MOBS.get()?.kind(mob)? {
            "saintsdragons:ignivorus" => Self {
                territory: 64.0,
                return_radius: 48.0,
                sleep_radius: 6.0,
                fall_asleep: 38,
                wake_up: 38,
            },
            "saintsdragons:varasuchus" => Self {
                territory: 48.0,
                return_radius: 32.0,
                sleep_radius: 3.0,
                fall_asleep: 42,
                wake_up: 129,
            },
            _ => return None,
        })
    }
}

struct Return {
    roost: Roost,
    profile: Profile,
    seen: u64,
    next_path: u64,
    settled_since: Option<u64>,
    waking_since: Option<u64>,
}

impl Return {
    fn wake(&mut self, now: u64, immediate: bool) {
        if immediate {
            self.settled_since = None;
            self.waking_since = None;
        } else if self.clip(now).is_some() {
            self.waking_since.get_or_insert(now);
        } else {
            self.settled_since = None;
        }
    }

    fn clip(&self, now: u64) -> Option<&'static str> {
        if self.waking_since.is_some() {
            return Some("wake_up");
        }
        let elapsed = now.saturating_sub(self.settled_since?);
        match elapsed {
            0..SETTLE_TICKS => None,
            _ if elapsed < SETTLE_TICKS + self.profile.fall_asleep => Some("fall_asleep"),
            _ => Some("sleep"),
        }
    }
}

fn block(position: [f64; 3]) -> [i32; 3] {
    let block = flight::cell(position);
    [block.x, block.y, block.z]
}

fn horizontal_distance(position: [f64; 3], home: [i32; 3]) -> f64 {
    (position[0] - (f64::from(home[0]) + 0.5)).hypot(position[2] - (f64::from(home[2]) + 0.5))
}

fn night(world: World<'_>) -> bool {
    world
        .day_time()
        .is_some_and(|time| !time.fixed_time && time.ticks.rem_euclid(24000) >= 12000)
}

pub fn set_home(mob: Entity<'_>, position: [f64; 3]) {
    Roost {
        home: block(position),
        anchor: None,
    }
    .save(mob);
}

pub fn sleep_clip(id: i32) -> Option<&'static str> {
    let active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    active.as_ref()?.get(&id)?.clip(tick_count())
}

/// DragonSleepComponent.startSleepExit / wakeUpImmediately and suppressSleep.
pub fn wake(mob: Entity<'_>, immediate: bool, suppression: u64) {
    if Profile::for_mob(mob).is_none() {
        return;
    }
    let now = tick_count();
    let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(state) = active.as_mut().and_then(|all| all.get_mut(&mob.id())) {
        state.wake(now, immediate);
    }
    let mut suppressed = SUPPRESSED.lock().unwrap_or_else(PoisonError::into_inner);
    let until = suppressed
        .get_or_insert_default()
        .entry(mob.id())
        .or_default();
    *until = (*until).max(now + suppression);
}

fn suppressed(id: i32, now: u64) -> bool {
    SUPPRESSED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|all| all.get(&id))
        .is_some_and(|until| now < *until)
}

pub fn forget(id: i32) {
    if let Some(active) = ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        active.remove(&id);
    }
    if let Some(suppressed) = SUPPRESSED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        suppressed.remove(&id);
    }
}

pub fn sweep() {
    let now = tick_count();
    if now.is_multiple_of(200)
        && let Some(active) = ACTIVE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        active.retain(|_, state| now.saturating_sub(state.seen) < 400);
    }
    if now.is_multiple_of(200)
        && let Some(all) = SUPPRESSED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        all.retain(|_, until| *until > now);
    }
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let Some(ai) = MOB_AI.get() else { return false };
    let id = mob.id();
    let now = tick_count();
    if phase == goal_phase::CAN_START && !(now + id as u64).is_multiple_of(CHECK_INTERVAL) {
        return false;
    }

    let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    let active = active.get_or_insert_default();
    if phase == goal_phase::STOP {
        active.remove(&id);
        ai.stop(mob);
        return false;
    }

    let Some(view) = mob.view() else { return false };
    if matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE)
        && (world.dimension_id() != Some("minecraft:overworld") || taming::is_tamed(mob))
    {
        return false;
    }

    let is_night = night(world);
    if phase == goal_phase::CAN_START {
        if ai.target(mob).is_some() || suppressed(id, now) {
            return false;
        }
        let Some((mut roost, profile)) = mob
            .data(DATA)
            .and_then(|nbt| fastnbt::from_bytes::<Roost>(&nbt).ok())
            .zip(Profile::for_mob(mob))
        else {
            return false;
        };
        if !is_night && horizontal_distance(view.position, roost.home) <= profile.territory {
            return false;
        }
        if roost.anchor.is_none() {
            roost.resolve_anchor(world, mob);
        }
        active.insert(
            id,
            Return {
                roost,
                profile,
                seen: now,
                next_path: now,
                settled_since: None,
                waking_since: None,
            },
        );
        return true;
    }

    let Some(state) = active.get_mut(&id) else {
        return false;
    };
    match phase {
        goal_phase::CONTINUE => {
            let target = ai.target(mob).is_some();
            let wet = matches!(
                world.fluid_id(flight::cell(view.position)).as_deref(),
                Some("minecraft:water" | "minecraft:flowing_water")
            );
            if state.clip(now).is_some() && (target || wet || !is_night) {
                state.wake(now, wet && !target);
                if target || wet {
                    let mut all = SUPPRESSED.lock().unwrap_or_else(PoisonError::into_inner);
                    let until = all.get_or_insert_default().entry(id).or_default();
                    *until = (*until).max(now + if target { 600 } else { 200 });
                }
            }
            if let Some(wake) = state.waking_since {
                if now.saturating_sub(wake) < state.profile.wake_up {
                    return true;
                }
                state.waking_since = None;
                state.settled_since = None;
            }
            if target || suppressed(id, now) {
                return false;
            }
            is_night
                || horizontal_distance(view.position, state.roost.home)
                    > state.profile.return_radius
        }
        goal_phase::START => {
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            state.seen = now;
            if state.waking_since.is_some() {
                ai.stop(mob);
                return false;
            }
            let sleep_site = state.roost.anchor.unwrap_or(state.roost.home);
            let distance = flight::length_squared(flight::offset(
                block(view.position).map(f64::from),
                sleep_site.map(f64::from),
            ));
            if is_night && distance <= state.profile.sleep_radius.powi(2) {
                if ai.navigating(mob) {
                    ai.stop(mob);
                }
                if view.on_ground
                    && world.fluid_id(flight::cell(view.position)).as_deref()
                        == Some("minecraft:empty")
                {
                    state.settled_since.get_or_insert(now);
                } else {
                    state.settled_since = None;
                }
                return false;
            }

            state.settled_since = None;
            if now >= state.next_path {
                state.next_path = now + 20;
                if is_night && state.roost.anchor.is_none() {
                    state.roost.resolve_anchor(world, mob);
                }
                let [x, y, z] = if is_night {
                    state.roost.anchor.unwrap_or(state.roost.home)
                } else {
                    state.roost.home
                }
                .map(f64::from);
                ai.walk_to(mob, [x + 0.5, y, z + 0.5], 1.0);
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varasuchus_sleep_refusal_covers_both_transitions() {
        let mut state = Return {
            roost: Roost {
                home: [0; 3],
                anchor: None,
            },
            profile: Profile {
                territory: 48.0,
                return_radius: 32.0,
                sleep_radius: 3.0,
                fall_asleep: 42,
                wake_up: 129,
            },
            seen: 0,
            next_path: 0,
            settled_since: Some(100),
            waking_since: None,
        };
        assert_eq!(state.clip(159), None);
        assert_eq!(state.clip(160), Some("fall_asleep"));
        assert_eq!(state.clip(201), Some("fall_asleep"));
        assert_eq!(state.clip(202), Some("sleep"));
        state.wake(300, false);
        assert_eq!(state.clip(300), Some("wake_up"));
        state.wake(350, false);
        assert_eq!(state.waking_since, Some(300));
        assert_eq!(state.clip(428), Some("wake_up"));
        state.wake(429, true);
        assert_eq!(state.clip(429), None);
    }
}
