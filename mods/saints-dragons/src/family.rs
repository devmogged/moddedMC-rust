//! DragonFollowParentBehaviour for the remaining species that register it.
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    entities::{ENTITY_ALIVE, Entity, EntityChange, SET_MOTION},
    mob_ai::goal_phase,
    tick_count,
};

use crate::{ENTITIES, MOB_AI, MOBS, cindervane_wild, eggs, flight, flight_route, taming};

pub const GOAL: u64 = 350;

pub fn speed(name: &str) -> Option<f64> {
    Some(match name {
        "raevyx" => 1.15,
        "ignivorus" | "varasuchus" => 1.1,
        "stegonaut" => 0.7,
        "nulljaw" => 0.9,
        _ => return None,
    })
}

#[derive(Clone, Copy)]
struct Follow {
    parent: i32,
    decision: u64,
    wander: u64,
}

static ACTIVE: LazyLock<Mutex<HashMap<i32, Follow>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn forget(id: i32) {
    ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
}

fn stop(mob: Entity<'_>, flying: bool) {
    if let Some(ai) = MOB_AI.get() {
        ai.stop(mob);
    }
    if flying {
        flight_route::forget(mob.id());
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion: [0.0; 3],
            ..EntityChange::default()
        });
    }
}

/// The released ground behavior updates its path every eight ticks and stops short of its
/// parent's body. Nulljaw follows the parent's center using the shared air waypoint planner.
pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(mobs), Some(ai), Some(entities), Some(view), Some(body)) = (
        MOBS.get(),
        MOB_AI.get(),
        ENTITIES.get(),
        mob.view(),
        mob.body(),
    ) else {
        return false;
    };
    let Some(kind) = mobs.kind(mob) else {
        return false;
    };
    let Some(name) = kind.strip_prefix("saintsdragons:") else {
        return false;
    };
    let Some(speed) = speed(name) else {
        return false;
    };
    let flying = name == "nulljaw";
    if phase == goal_phase::STOP {
        forget(mob.id());
        stop(mob, flying);
        return false;
    }
    if !eggs::is_juvenile(mob)
        || taming::is_tamed(mob)
        || !mobs.passengers(mob).is_empty()
        || mob.vehicle().is_some()
        || eggs::water_at(world, flight::cell(view.position))
        || ai.target(mob).is_some()
    {
        return false;
    }
    let now = tick_count();
    let mut state = if phase == goal_phase::CAN_START {
        let Some(parent) = cindervane_wild::find_parent(world, mob) else {
            return false;
        };
        Follow {
            parent,
            decision: now,
            wander: 0,
        }
    } else {
        let Some(state) = ACTIVE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&mob.id())
            .copied()
        else {
            return false;
        };
        state
    };
    let Some(Some((at, width, height))) = entities.with_entity(world, state.parent, |parent| {
        if mobs.kind(parent) != Some(kind)
            || parent.flags() & ENTITY_ALIVE == 0
            || eggs::is_juvenile(parent)
        {
            return None;
        }
        let body = parent.body()?;
        Some((
            parent.view()?.position,
            body.max[0] - body.min[0],
            body.max[1] - body.min[1],
        ))
    }) else {
        return false;
    };
    let minimum = cindervane_wild::parent_minimum(body.max[0] - body.min[0], width);
    let distance = flight::length_squared(flight::offset(at, view.position));
    match phase {
        goal_phase::CAN_START => {
            if distance < minimum {
                return false;
            }
            ACTIVE
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(mob.id(), state);
            true
        }
        goal_phase::CONTINUE => distance >= minimum && distance <= 576.0,
        goal_phase::TICK => {
            ai.look_at(mob, at);
            if distance < minimum {
                stop(mob, flying);
                state.wander = now + 40 + crate::ai::random(40);
            } else if now < state.wander {
                if distance > minimum * 2.0 {
                    state.wander = 0;
                }
            } else if now >= state.decision {
                state.decision = now + 8;
                if distance < minimum * 1.2 {
                    stop(mob, flying);
                    state.wander = now + 20 + crate::ai::random(20);
                } else if flying {
                    // NulljawFollowParentBehaviour uses the adult's midpoint.
                    let target = [at[0], at[1] + height * 0.5, at[2]];
                    let waypoint = flight_route::waypoint(world, mob, view.position, target, false);
                    let delta = flight::offset(waypoint, view.position);
                    let step = 0.42 * speed;
                    let motion =
                        delta.map(|v| v * step / flight::length_squared(delta).sqrt().max(step));
                    if flight::clear_body(world, body, motion) {
                        mobs.set_no_gravity(mob, true);
                        mob.reset_fall_distance();
                        mob.change(&EntityChange {
                            mask: SET_MOTION,
                            motion,
                            ..EntityChange::default()
                        });
                    }
                } else {
                    ai.walk_to(mob, at, speed);
                }
            }
            ACTIVE
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(mob.id(), state);
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_species_with_released_follow_parent_goals_are_registered() {
        assert_eq!(speed("nulljaw"), Some(0.9));
        assert_eq!(speed("stegonaut"), Some(0.7));
        assert_eq!(speed("raevyx"), Some(1.15));
        assert_eq!(speed("volitans"), None);
    }
}
