//! CindervanePackFlightCoordinator and local flight steering (0.9.88).
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    entities::{
        ENTITY_ALIVE, Entity, EntityBody, EntityChange, EntityView, SET_BODY_YAW, SET_HEAD_YAW,
        SET_MOTION, SET_YAW,
    },
    tick_count,
};

use crate::{
    CINDERVANE, ENTITIES, MOBS,
    flight::{self, add, length_squared, offset},
};

const UPDATE: u64 = 4;
const SLOT_LEASE: u64 = 100;
// The current Rust autonomous controller uses raw blocks/tick rather than Java's
// flight attribute multiplier. Scale its 1.25 cruise speed to the same basis.
const SPEED: f64 = 1.0;

#[derive(Clone, Copy)]
pub struct Leader {
    pub id: i32,
    pub view: EntityView,
    pub height: f64,
    pub airborne: bool,
}

#[derive(Clone, Copy)]
struct Slot {
    index: usize,
    seen: u64,
    catch_up: bool,
    velocity: [f64; 3],
    updated: u64,
}

#[derive(Default)]
struct Formation {
    heading: f64,
    velocity: [f64; 3],
    moving: bool,
    updated: Option<u64>,
    slots: HashMap<i32, Slot>,
}

#[derive(Default)]
struct State {
    formations: HashMap<i32, Formation>,
    active: HashMap<i32, bool>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(PoisonError::into_inner))
}

pub fn landing(id: i32) -> Option<bool> {
    with(|state| state.active.get(&id).copied())
}

pub fn stop(id: i32) {
    with(|state| {
        state.active.remove(&id);
    });
}

pub fn forget(id: i32) {
    with(|state| {
        state.active.remove(&id);
        state.formations.remove(&id);
        for formation in state.formations.values_mut() {
            formation.slots.remove(&id);
        }
    });
}

fn scale(v: [f64; 3], s: f64) -> [f64; 3] {
    v.map(|c| c * s)
}

fn length(v: [f64; 3]) -> f64 {
    length_squared(v).sqrt()
}

fn limit(v: [f64; 3], max: f64) -> [f64; 3] {
    let len = length(v);
    if len > max { scale(v, max / len) } else { v }
}

fn wrap(degrees: f64) -> f64 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// CindervanePackFlightCoordinator.updateLeader: damp changes of course and velocity.
impl Formation {
    fn update(&mut self, view: EntityView, now: u64) {
        if self.updated == Some(now) {
            return;
        }
        let elapsed = self.updated.map_or(1, |last| now.saturating_sub(last));
        let reset = self.updated.is_none() || elapsed > 20;
        let [x, _, z] = view.motion;
        let yaw = if x * x + z * z > 0.01 {
            (-x).atan2(z).to_degrees()
        } else if reset {
            f64::from(view.yaw)
        } else {
            self.heading
        };
        if reset {
            self.heading = yaw;
            self.velocity = view.motion;
        } else {
            let turn = 3.0 * elapsed as f64;
            self.heading += wrap(yaw - self.heading).clamp(-turn, turn);
            let blend = 1.0 - 0.8_f64.powf(elapsed as f64);
            self.velocity = add(
                self.velocity,
                scale(offset(view.motion, self.velocity), blend),
            );
        }
        self.moving = length_squared(self.velocity) > if self.moving { 0.0025 } else { 0.01 };
        self.updated = Some(now);
    }

    fn slot(&mut self, id: i32, now: u64) -> &mut Slot {
        // shortcut: lease inactive slots instead of resolving every stored UUID each cleanup;
        // replace when the host provides pack lifecycle callbacks for unloaded entities.
        self.slots
            .retain(|_, slot| now.saturating_sub(slot.seen) <= SLOT_LEASE);
        let index = (0..)
            .find(|index| self.slots.values().all(|slot| slot.index != *index))
            .unwrap();
        let slot = self.slots.entry(id).or_insert(Slot {
            index,
            seen: now,
            catch_up: false,
            velocity: [0.0; 3],
            updated: now.saturating_sub(UPDATE),
        });
        slot.seen = now;
        slot
    }
}

/// CindervanePackFlightCoordinator.slotPosition: two wings and a rear slot per row.
fn slot_position(at: [f64; 3], height: f64, yaw: f64, index: usize) -> [f64; 3] {
    let row = index / 3;
    let side = index % 3;
    let lateral = match side {
        0 => -7.0,
        1 => 7.0,
        _ => 0.0,
    } * (row + 1) as f64;
    let trailing = 10.0 + (row * 2 + usize::from(side == 2)) as f64 * 8.0;
    let (sin, cos) = yaw.to_radians().sin_cos();
    add(
        at,
        [
            sin * trailing - cos * lateral,
            height + 1.5 + row as f64,
            -cos * trailing - sin * lateral,
        ],
    )
}

fn formation_velocity(
    error: [f64; 3],
    leader: [f64; 3],
    moving: bool,
    separation: [f64; 3],
) -> [f64; 3] {
    let distance = length(error);
    let correction = if distance > 1.5 {
        scale(
            error,
            ((distance - 1.5) * 0.065).min(SPEED * 0.75) / distance,
        )
    } else {
        [0.0; 3]
    };
    let mut velocity = add(leader, correction);
    let horizontal = leader[0].hypot(leader[2]);
    if moving && horizontal > 0.05 {
        let forward = [leader[0] / horizontal, 0.0, leader[2] / horizontal];
        let forward_speed = velocity[0] * forward[0] + velocity[2] * forward[2];
        velocity = add(
            velocity,
            scale(forward, (horizontal * 0.2 - forward_speed).max(0.0)),
        );
    }
    limit(add(velocity, separation), SPEED * 1.65)
}

fn separation(world: World<'_>, mob: Entity<'_>, at: [f64; 3], body: EntityBody) -> [f64; 3] {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return [0.0; 3];
    };
    let mut sum = [0.0; 3];
    let width = body.max[0] - body.min[0];
    for id in entities.entities_in(world, at.map(|c| c - 8.0), at.map(|c| c + 8.0), mob.id()) {
        let Some(Some(away)) = entities.with_entity(world, id, |other| {
            if mobs.kind(other) != Some(CINDERVANE) || other.flags() & ENTITY_ALIVE == 0 {
                return None;
            }
            let view = other.view()?;
            if view.on_ground {
                return None;
            }
            let body = other.body()?;
            let clearance = ((width + body.max[0] - body.min[0]) * 0.5 + 2.0).min(8.0);
            let delta = offset(at, view.position);
            let dist = length(delta);
            if dist >= clearance {
                return None;
            }
            let direction = if dist > 1.0e-4 {
                scale(delta, 1.0 / dist)
            } else {
                [if mob.id() < id { -1.0 } else { 1.0 }, 0.0, 0.0]
            };
            Some(scale(direction, (1.0 - dist / clearance).powi(2)))
        }) else {
            continue;
        };
        sum = add(sum, away);
    }
    scale(limit(sum, 1.0), SPEED * 0.5)
}

/// The host has block collision queries but no asynchronous flight navigator.
/// Try direct motion, side steps and a climb, then brake rather than enter a solid/unloaded cell.
fn steer(world: World<'_>, body: EntityBody, desired: [f64; 3]) -> [f64; 3] {
    let clear = |v| (1..=4).all(|i| flight::clear_body(world, body, scale(v, f64::from(i) / 4.0)));
    if clear(desired) {
        return desired;
    }
    let [x, y, z] = desired;
    for angle in [45.0_f64, -45.0, 90.0, -90.0] {
        let (s, c) = angle.to_radians().sin_cos();
        let alternative = [x * c - z * s, y.max(0.1), x * s + z * c];
        if clear(alternative) {
            return alternative;
        }
    }
    for candidate in [[0.0, 0.45, 0.0], scale(desired, 0.5), scale(desired, 0.25)] {
        if clear(candidate) {
            return candidate;
        }
    }
    [0.0; 3]
}

fn move_dragon(mob: Entity<'_>, view: EntityView, motion: [f64; 3]) {
    let yaw = if motion[0].hypot(motion[2]) > 0.01 {
        f64::from(view.yaw)
            + wrap((-motion[0]).atan2(motion[2]).to_degrees() - f64::from(view.yaw))
                .clamp(-12.0, 12.0)
    } else {
        f64::from(view.yaw)
    } as f32;
    mob.reset_fall_distance();
    mob.change(&EntityChange {
        mask: SET_MOTION | SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        motion,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..EntityChange::default()
    });
}

/// CindervanePackFollowBehaviour.followAirFormation, updated every four ticks.
pub fn follow(world: World<'_>, mob: Entity<'_>, view: EntityView, leader: Leader) {
    let Some(body) = mob.body() else { return };
    let now = tick_count();
    let refresh = with(|state| {
        state.active.insert(mob.id(), false);
        state.formations.retain(|_, f| {
            f.updated
                .is_some_and(|t| now.saturating_sub(t) <= SLOT_LEASE)
        });
        let formation = state.formations.entry(leader.id).or_default();
        formation.update(leader.view, now);
        let slot = formation.slot(mob.id(), now);
        now.saturating_sub(slot.updated) >= UPDATE || slot.updated == 0
    });
    let repel = if refresh {
        separation(world, mob, view.position, body)
    } else {
        [0.0; 3]
    };
    let desired = with(|state| {
        let formation = state.formations.get_mut(&leader.id).unwrap();
        let (heading, velocity, moving) = (formation.heading, formation.velocity, formation.moving);
        let slot = formation.slot(mob.id(), now);
        if refresh {
            let error = offset(
                slot_position(leader.view.position, leader.height, heading, slot.index),
                view.position,
            );
            slot.catch_up = length(error) > if slot.catch_up { 10.0 } else { 18.0 };
            slot.velocity = formation_velocity(error, velocity, moving, repel);
            slot.updated = now;
        }
        slot.velocity
    });
    // The carrier is subject to ordinary gravity. A small upward correction supports
    // hover; takeoff climbs before lateral travel, as the Java takeoff controller does.
    let desired = if view.on_ground {
        [0.0, 0.45, 0.0]
    } else {
        [
            desired[0],
            (desired[1] + 0.08).clamp(-0.42, 0.45),
            desired[2],
        ]
    };
    move_dragon(mob, view, steer(world, body, desired));
}

/// Transition back to ground when the leader lands or disappears.
pub fn land(world: World<'_>, mob: Entity<'_>, view: EntityView, leader: Option<Leader>) {
    if view.on_ground {
        stop(mob.id());
        return;
    }
    with(|state| {
        state.active.insert(mob.id(), true);
    });
    let Some(body) = mob.body() else { return };
    let mut motion = [0.0, -0.22, 0.0];
    if let Some(leader) = leader {
        let [x, _, z] = offset(leader.view.position, view.position);
        if x.hypot(z) > 10.0 {
            let horizontal = limit([x, 0.0, z], 0.75);
            if flight::clear_body(world, body, horizontal) {
                motion[0] = horizontal[0];
                motion[2] = horizontal[2];
            }
        }
    }
    move_dragon(mob, view, motion);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_distinct_and_rotate_with_leader() {
        assert_eq!(slot_position([0.0; 3], 2.0, 0.0, 0), [7.0, 3.5, -10.0]);
        assert_eq!(slot_position([0.0; 3], 2.0, 0.0, 1), [-7.0, 3.5, -10.0]);
        assert_eq!(slot_position([0.0; 3], 2.0, 0.0, 2), [0.0, 3.5, -18.0]);
        let turned = slot_position([0.0; 3], 2.0, 90.0, 0);
        assert!(length(offset(turned, [10.0, 3.5, 7.0])) < 1.0e-9);
    }

    #[test]
    fn existing_members_keep_slots_and_expired_slots_are_reused() {
        let mut f = Formation::default();
        assert_eq!(f.slot(1, 4).index, 0);
        assert_eq!(f.slot(2, 4).index, 1);
        assert_eq!(f.slot(2, 100).index, 1);
        assert_eq!(f.slot(3, 105).index, 0);
        assert_eq!(f.slot(2, 105).index, 1);
    }

    #[test]
    fn leader_heading_turns_gradually_and_velocity_is_smoothed() {
        let mut f = Formation::default();
        f.update(
            EntityView {
                motion: [0.0, 0.0, 1.0],
                ..EntityView::default()
            },
            1,
        );
        f.update(
            EntityView {
                motion: [-1.0, 0.0, 0.0],
                ..EntityView::default()
            },
            5,
        );
        assert_eq!(f.heading, 12.0);
        assert!((f.velocity[0] + 0.5904).abs() < 1e-9);
    }

    #[test]
    fn correction_does_not_reverse_a_cruising_follower_and_speed_is_capped() {
        let motion = formation_velocity([0.0, 0.0, -100.0], [0.0, 0.0, 0.5], true, [0.0; 3]);
        assert!((motion[2] - 0.1).abs() < 1e-9);
        assert!(
            length(formation_velocity(
                [100.0; 3],
                [0.0, 0.0, 1.25],
                true,
                [0.5; 3]
            )) <= 1.65 + 1e-9
        );
        assert_eq!(
            formation_velocity([1.0, 0.0, 0.0], [0.0; 3], false, [0.0; 3]),
            [0.0; 3]
        );
    }
}
