//! Suspended wandering for released Draconian Swarm members.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    entities::{Entity, EntityChange, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION, SET_YAW},
    mob_ai::goal_phase,
    tick_count,
};

use crate::{
    MOB_AI, MOBS,
    ai::random,
    flight::{add, cell, clear_body, length_squared, offset},
};

pub const WANDER: u64 = 10;
pub const MEMBERS: [(&str, f64); 3] = [("latcher", 4.0), ("winged", 1.5), ("whettled", 4.0)];
const WANDER_INTERVAL: u64 = 32;
const ARRIVAL: f64 = 0.6;

#[derive(Clone, Copy)]
struct Course {
    at: [f64; 3],
    until: u64,
}

static COURSES: Mutex<Option<HashMap<i32, Course>>> = Mutex::new(None);

pub fn damage(name: &str) -> Option<f64> {
    MEMBERS
        .iter()
        .find_map(|(member, damage)| (*member == name).then_some(*damage))
}

pub fn forget(id: i32) {
    if let Some(courses) = COURSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        courses.remove(&id);
    }
}

pub fn hover(mob: Entity<'_>, motion: [f64; 3]) {
    if let Some(mobs) = MOBS.get() {
        mobs.set_no_gravity(mob, true);
    }
    let active = COURSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|courses| courses.contains_key(&mob.id()));
    if !active && motion != [0.0; 3] {
        let motion = if length_squared(motion) < 0.0001 {
            [0.0; 3]
        } else {
            motion.map(|speed| speed * 0.86)
        };
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion,
            ..EntityChange::default()
        });
    }
}

pub fn wander(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase == goal_phase::STOP {
        forget(mob.id());
        return false;
    }
    if phase == goal_phase::CAN_START && random(WANDER_INTERVAL) != 0 {
        return false;
    }
    let (Some(view), Some(ai)) = (mob.view(), MOB_AI.get()) else {
        return false;
    };
    if ai.target(mob).is_some() {
        return false;
    }
    let now = tick_count();
    if phase == goal_phase::CAN_START {
        let Some(at) = destination(world, mob, view.position, view.yaw) else {
            return false;
        };
        let mut locked = COURSES.lock().unwrap_or_else(PoisonError::into_inner);
        let courses = locked.get_or_insert_default();
        courses.retain(|_, course| course.until > now);
        courses.insert(
            mob.id(),
            Course {
                at,
                until: now + 200,
            },
        );
        return true;
    }
    let course = COURSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|courses| courses.get(&mob.id()))
        .copied();
    let Some(course) = course else {
        return false;
    };
    let delta = offset(course.at, view.position);
    let distance = length_squared(delta).sqrt();
    if phase == goal_phase::CONTINUE {
        return course.until > now && distance > ARRIVAL;
    }
    if phase != goal_phase::TICK || distance <= ARRIVAL {
        return false;
    }
    let speed = match MOBS.get().and_then(|mobs| mobs.kind(mob)) {
        Some("saintsdragons:latcher") => 0.34,
        Some("saintsdragons:winged") => 0.43,
        Some("saintsdragons:whettled") => 0.38,
        _ => return false,
    } * (distance / (ARRIVAL * 2.5)).clamp(0.25, 1.0);
    let motion = std::array::from_fn(|axis| {
        let weight = if axis == 1 { 0.12 } else { 0.16 };
        view.motion[axis] + weight * (delta[axis] * speed / distance - view.motion[axis])
    });
    if !mob
        .body()
        .is_some_and(|body| clear_body(world, body, motion))
    {
        forget(mob.id());
        return false;
    }
    let heading = -motion[0].atan2(motion[2]).to_degrees() as f32;
    let turn = (heading - view.yaw + 180.0).rem_euclid(360.0) - 180.0;
    let yaw = view.yaw + turn.clamp(-7.0, 7.0);
    mob.change(&EntityChange {
        mask: SET_MOTION | SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        motion,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..EntityChange::default()
    });
    true
}

// shortcut: routes are straight, use waypoint search when combat needs turns around obstacles.
fn destination(world: World<'_>, mob: Entity<'_>, from: [f64; 3], yaw: f32) -> Option<[f64; 3]> {
    let body = mob.body()?;
    for _ in 0..4 {
        let heading = f64::from(yaw).to_radians() + (random(181) as f64 - 90.0).to_radians();
        let distance = (4 + random(13)) as f64;
        let delta = [
            -heading.sin() * distance,
            random(17) as f64 - 8.0,
            heading.cos() * distance,
        ];
        let at = add(from, delta);
        if world.fluid_id(cell(at)).as_deref() != Some("minecraft:empty") {
            continue;
        }
        let steps = length_squared(delta).sqrt().ceil() as u32;
        if (1..=steps).all(|step| {
            clear_body(
                world,
                body,
                delta.map(|v| v * f64::from(step) / f64::from(steps)),
            )
        }) {
            return Some(at);
        }
    }
    None
}
