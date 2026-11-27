//! Moop.registerGoals: independent swimming and panic, without cod schooling or avoidance.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::World,
    entities::Entity,
    mob_ai::{Goal, goal_control, goal_phase},
};
use serde::Deserialize;

use crate::{MOB_AI, ai, flight};

pub const SWIM: u64 = 180;
pub const PANIC: u64 = 181;
const KIND: &str = "saintsdragons:moop";
const SWIM_SPEED: f64 = 1.0;
const PANIC_SPEED: f64 = 1.25;
// RandomSwimmingGoal(1.0, 30) uses reducedTickDelay(30) on the half-rate selector.
const SWIM_INTERVAL: u64 = 15;
const ATTEMPTS: usize = 10;

// Only CAN_START -> START needs to carry the chosen path destination.
type Pending = HashMap<(i32, u64), [f64; 3]>;
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let ai = MOB_AI.get().ok_or(RegistrationError::InvalidDefinition)?;
    ai.replace_goals(registry, KIND)?;
    for (key, priority) in [(SWIM, 0), (PANIC, 2)] {
        ai.add_goal(
            registry,
            &Goal {
                kind: KIND,
                key,
                priority,
                controls: goal_control::MOVE,
            },
        )?;
    }
    Ok(())
}

fn water(world: World<'_>, pos: Pos) -> bool {
    matches!(
        world.fluid_id(pos).as_deref(),
        Some("minecraft:water" | "minecraft:flowing_water")
    ) && world
        .collision_shape(pos)
        .is_some_and(|shape| shape.is_empty())
}

fn center(pos: Pos) -> [f64; 3] {
    [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    ]
}

/// BehaviorUtils.getRandomSwimmablePos / PanicGoal.findRandomPosition.
/// shortcut: use loaded collision-free water cells rather than vanilla's path malus and
/// walk-target scoring; replace when the host exposes DefaultRandomPos / random swim positions.
fn random_target(
    origin: Pos,
    horizontal: i32,
    vertical: i32,
    mut random: impl FnMut(u64) -> u64,
    mut swimmable: impl FnMut(Pos) -> bool,
) -> Option<[f64; 3]> {
    for _ in 0..ATTEMPTS {
        let mut offset = |range: i32| random((2 * range + 1) as u64) as i32 - range;
        let pos = Pos {
            x: origin.x + offset(horizontal),
            y: origin.y + offset(vertical),
            z: origin.z + offset(horizontal),
        };
        if swimmable(pos) {
            return Some(center(pos));
        }
    }
    None
}

/// PanicGoal.lookForWater: nearest water within 5 horizontal and 1 vertical blocks.
fn nearest_water(origin: Pos, mut swimmable: impl FnMut(Pos) -> bool) -> Option<[f64; 3]> {
    let mut best = None;
    for dx in -5_i32..=5 {
        for dy in -1_i32..=1 {
            for dz in -5_i32..=5 {
                let distance = dx.abs() + dy.abs() + dz.abs();
                let pos = Pos {
                    x: origin.x + dx,
                    y: origin.y + dy,
                    z: origin.z + dz,
                };
                if best.is_none_or(|(score, _)| distance < score) && swimmable(pos) {
                    best = Some((distance, pos));
                }
            }
        }
    }
    best.map(|(_, pos)| center(pos))
}

#[derive(Deserialize)]
struct Fire {
    #[serde(rename = "Fire", default)]
    ticks: i16,
}

fn burning(mob: Entity<'_>) -> bool {
    // The host has no cheap fire-tick getter. The already-required storage interface can
    // read the vanilla saved Fire field; only do so out of water, which extinguishes fish.
    moddedmc_api::mobs::require_storage()
        .ok()
        .and_then(|storage| storage.save(mob))
        .and_then(|nbt| fastnbt::from_bytes::<Fire>(&nbt).ok())
        .is_some_and(|fire| fire.ticks > 0)
}

pub fn goal(phase: u32, key: u64, world: World<'_>, mob: Entity<'_>) -> bool {
    let Some(ai) = MOB_AI.get() else { return false };
    match phase {
        goal_phase::CAN_START => {
            let Some(view) = mob.view() else { return false };
            let origin = flight::cell(view.position);
            let target = if key == SWIM {
                // shortcut: no_action_time and controlling passenger aren't exposed by the
                // host; Moop isn't rideable and the carrier still handles natural despawn.
                if ai::random(SWIM_INTERVAL) != 0 {
                    return false;
                }
                random_target(origin, 10, 7, ai::random, |pos| water(world, pos))
            } else {
                let fire = !water(world, origin) && burning(mob);
                if !fire && ai.last_hurt_by(mob).is_none() {
                    return false;
                }
                if fire {
                    nearest_water(origin, |pos| water(world, pos))
                } else {
                    None
                }
                .or_else(|| random_target(origin, 5, 4, ai::random, |pos| water(world, pos)))
            };
            let Some(target) = target else { return false };
            PENDING
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_default()
                .insert((mob.id(), key), target);
            true
        }
        goal_phase::CONTINUE => ai.navigating(mob),
        goal_phase::START => {
            let target = PENDING
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_default()
                .remove(&(mob.id(), key));
            if let Some(target) = target {
                ai.walk_to(
                    mob,
                    target,
                    if key == SWIM { SWIM_SPEED } else { PANIC_SPEED },
                );
            }
            false
        }
        goal_phase::STOP => {
            ai.stop(mob);
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swimming_retries_and_centers_the_first_water_cell() {
        let origin = Pos {
            x: 10,
            y: 20,
            z: 30,
        };
        let mut values = [0, 0, 0, 20, 14, 20].into_iter();
        let mut checked = Vec::new();
        let result = random_target(
            origin,
            10,
            7,
            |bound| {
                let value = values.next().unwrap();
                assert!(value < bound);
                value
            },
            |pos| {
                checked.push((pos.x, pos.y, pos.z));
                pos.x == 20
            },
        );
        assert_eq!(checked, [(0, 13, 20), (20, 27, 40)]);
        assert_eq!(result, Some([20.5, 27.0, 40.5]));
        assert_eq!(random_target(origin, 5, 4, |_| 0, |_| false), None);
    }

    #[test]
    fn panic_prefers_nearest_water_within_the_released_search_box() {
        let origin = Pos { x: 0, y: 0, z: 0 };
        let target = nearest_water(origin, |pos| {
            matches!((pos.x, pos.y, pos.z), (5, 1, 5) | (0, 2, 0) | (1, -1, 0))
        });
        assert_eq!(target, Some([1.5, -1.0, 0.5]));
        assert_eq!(nearest_water(origin, |pos| pos.y == 2), None);
    }
}
