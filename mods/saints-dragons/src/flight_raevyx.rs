//! RaevyxAutonomousFlightBehaviour and DragonFlightSpace (0.9.88).
use super::{Flight, FlightState, cell, length_squared, offset};
use crate::{MOB_AI, MOBS, ai::random, taming};
use moddedmc_api::{
    Pos,
    content::World,
    entities::{
        Entity, EntityBody, EntityChange, EntityView, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION,
        SET_YAW,
    },
    mob_ai::goal_phase,
};

const CRUISE_SPEED: f64 = 2.6;
const TARGET_AGE: u64 = 300;
const REACHED_SQ: f64 = 64.0;

#[derive(Clone, Copy)]
struct Profile {
    landing_cooldown: u64,
    decision: u64,
    takeoff: u64,
    keep: u64,
    max_height: f64,
}

/// DragonFlightBehaviorProfile.raevyx, with the separate rain height from the behaviour.
fn profile(thunder: bool, rain: bool) -> Profile {
    if thunder {
        Profile {
            landing_cooldown: 0,
            decision: 2,
            takeoff: 4,
            keep: 3000,
            max_height: 90.0,
        }
    } else {
        Profile {
            landing_cooldown: 100,
            decision: 25,
            takeoff: 80,
            keep: 200,
            max_height: if rain { 70.0 } else { 50.0 },
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Cruise {
    target: [f64; 3],
    since: u64,
    dive: bool,
}

fn random_unit() -> f64 {
    random(1 << 24) as f64 / (1 << 24) as f64
}

/// A stationary carrier can report airborne while its feet are resting on a block:
/// gravity is applied after collision and the landing controller may zero its velocity.
/// Recognize actual supporting geometry so a blocked takeoff can retry from the ground.
fn supports(body: EntityBody, min: [f64; 3], max: [f64; 3]) -> bool {
    (body.min[1] - max[1]).abs() <= 1e-3
        && [0, 2]
            .into_iter()
            .all(|i| max[i] > body.min[i] && min[i] < body.max[i])
}

fn grounded(world: World<'_>, mob: Entity<'_>, view: EntityView) -> bool {
    if view.on_ground {
        return true;
    }
    if view.motion[1] > 0.0 {
        return false;
    }
    let Some(body) = mob.body() else { return false };
    let lo = cell(body.min);
    let hi = cell(body.max.map(|v| v - 1e-4));
    let y = (body.min[1] - 1e-3).floor() as i32;
    for x in lo.x..=hi.x {
        for z in lo.z..=hi.z {
            let Some(shapes) = world.collision_shape(Pos { x, y, z }) else {
                continue;
            };
            let origin = [f64::from(x), f64::from(y), f64::from(z)];
            if shapes.iter().any(|shape| {
                supports(
                    body,
                    super::add(origin, shape.min),
                    super::add(origin, shape.max),
                )
            }) {
                return true;
            }
        }
    }
    false
}

fn clear(world: World<'_>, body: EntityBody, shift: [f64; 3], travel: [f64; 3]) -> bool {
    let min =
        std::array::from_fn::<_, 3, _>(|i| body.min[i] + shift[i] + travel[i].min(0.0) + 1e-4);
    let max =
        std::array::from_fn::<_, 3, _>(|i| body.max[i] + shift[i] + travel[i].max(0.0) - 1e-4);
    let lo = cell(min);
    let hi = cell(max);
    for x in lo.x..=hi.x {
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                let pos = Pos { x, y, z };
                let Some(shapes) = world.collision_shape(pos) else {
                    return false;
                };
                let origin = [f64::from(x), f64::from(y), f64::from(z)];
                if shapes.iter().any(|shape| {
                    (0..3).all(|i| {
                        origin[i] + shape.max[i] > min[i] && origin[i] + shape.min[i] < max[i]
                    })
                }) || world.fluid_id(pos).as_deref() != Some("minecraft:empty")
                {
                    return false;
                }
            }
        }
    }
    true
}

/// DragonFlightSpace.corridorClear. A conservative swept box every four blocks.
fn corridor(world: World<'_>, body: EntityBody, from: [f64; 3], to: [f64; 3]) -> bool {
    let delta = offset(to, from);
    let steps = (length_squared(delta).sqrt() / 4.0).ceil().max(1.0) as u32;
    let step = delta.map(|v| v / f64::from(steps));
    (0..steps).all(|i| clear(world, body, step.map(|v| v * f64::from(i)), step))
}

struct Space {
    floor: Option<f64>,
    ceiling: Option<f64>,
}

/// shortcut: vertical limits use the center column, while the full carrier body is checked
/// along the corridor. Upgrade to the released cached full-body flight-space planner.
fn observe(world: World<'_>, at: [f64; 3], height: f64) -> Option<Space> {
    let bounds = moddedmc_api::worldgen_lookup::require()
        .ok()?
        .height_bounds(world)?;
    let pos = cell(at);
    let mut floor = None;
    let mut ceiling = None;
    for y in ((pos.y - 32).max(bounds.0)..=pos.y).rev() {
        for shape in world.collision_shape(Pos { y, ..pos })? {
            let top = f64::from(y) + shape.max[1];
            if top <= at[1] + 1e-4 {
                floor = Some(floor.map_or(top, |old: f64| old.max(top)));
            }
        }
        if floor.is_some() {
            break;
        }
    }
    let top = (at[1] + height).floor() as i32;
    for y in top..=(top + 32).min(bounds.1 - 1) {
        for shape in world.collision_shape(Pos { y, ..pos })? {
            let bottom = f64::from(y) + shape.min[1];
            if bottom >= at[1] + height - 1e-4 {
                ceiling = Some(ceiling.map_or(bottom, |old: f64| old.min(bottom)));
            }
        }
        if ceiling.is_some() {
            break;
        }
    }
    Some(Space { floor, ceiling })
}

fn fit_height(wanted: f64, space: &Space, height: f64) -> Option<f64> {
    let min = space.floor.map_or(f64::NEG_INFINITY, |y| y + 1.5);
    let max = space.ceiling.map_or(f64::INFINITY, |y| y - height - 1.5);
    (min <= max).then(|| wanted.clamp(min, max))
}

/// AutonomousFlightBehaviour.buildAutonomousDiveTarget.
fn dive_height(current_y: f64, floor: f64, distance: f64) -> Option<f64> {
    let min = floor + 10.0;
    let max = current_y - 2.0;
    (distance >= 8.0 && min <= max)
        .then(|| (current_y - distance * 34.0_f64.to_radians().tan()).clamp(min, max))
}

fn cruise_target(
    world: World<'_>,
    mob: Entity<'_>,
    view: EntityView,
    p: Profile,
    now: u64,
    wider: bool,
) -> Option<Cruise> {
    let body = mob.body()?;
    let height = body.max[1] - body.min[1];
    let at = view.position;
    let local = observe(world, at, height)?;
    let enclosed = local.ceiling.is_some();
    let wanted = local.floor.map_or(at[1], |floor| {
        floor
            + p.max_height.min(if enclosed {
                5.0
            } else {
                15.0 + random_unit() * 20.0
            })
    });
    let wanted = fit_height(wanted.clamp(at[1] - 6.0, at[1] + 6.0), &local, height)?;
    for attempt in 0..16 {
        let nearby = enclosed || attempt >= 8;
        let range = if nearby {
            6.0 + random_unit() * 18.0
        } else {
            50.0 + random_unit() * 80.0
        };
        let turn = if nearby || wider { 360.0 } else { 180.0 };
        let angle = (f64::from(view.yaw) + (random_unit() - 0.5) * turn).to_radians();
        let mut target = [
            at[0] - angle.sin() * range,
            wanted + (random_unit() - 0.5) * 3.0,
            at[2] + angle.cos() * range,
        ];
        let Some(there) = observe(world, target, height) else {
            continue;
        };
        let Some(y) = fit_height(target[1], &there, height) else {
            continue;
        };
        if (y - at[1]).abs() > 8.0 {
            continue;
        }
        target[1] = y;
        if (view.on_ground && !corridor(world, body, at, target))
            || (!view.on_ground && !clear(world, body, offset(target, at), [0.0; 3]))
        {
            continue;
        }
        let mut dive = false;
        if !view.on_ground
            && world.weather_state().is_some_and(|w| !w.raining)
            && !enclosed
            && local.floor.is_some_and(|floor| at[1] - floor >= 28.0)
            && there.ceiling.is_none()
            && random(3) == 0
            && let Some(y) = there
                .floor
                .and_then(|floor| dive_height(at[1], floor, range))
        {
            let diving = [target[0], y, target[2]];
            if corridor(world, body, at, diving) {
                target = diving;
                dive = true;
            }
        }
        return Some(Cruise {
            target,
            since: now,
            dive,
        });
    }
    None
}

/// RideableFlyingDragon.isOverStandardFlightDanger.
fn danger(world: World<'_>, at: [f64; 3]) -> bool {
    let pos = cell(at);
    let mut near_fluid = false;
    for i in 1..=25 {
        let p = Pos {
            y: pos.y - i,
            ..pos
        };
        let Some(shape) = world.collision_shape(p) else {
            return near_fluid;
        };
        if !shape.is_empty() || world.face_sturdy(p, 1) {
            return near_fluid;
        }
        if i <= 10 && world.fluid_id(p).is_some_and(|id| id != "minecraft:empty") {
            near_fluid = true;
        }
    }
    near_fluid
        || moddedmc_api::worldgen_lookup::require()
            .ok()
            .and_then(|lookup| lookup.height_bounds(world))
            .is_some_and(|(min, _)| pos.y < min + 20)
}

fn steer(
    world: World<'_>,
    mob: Entity<'_>,
    view: EntityView,
    target: [f64; 3],
    speed: f64,
    route: bool,
) -> bool {
    let Some(body) = mob.body() else { return false };
    let destination = target;
    let target = if route {
        crate::flight_route::waypoint(world, mob, view.position, destination, false)
    } else {
        destination
    };
    let delta = offset(target, view.position);
    let length = length_squared(delta).sqrt();
    let motion = delta.map(|v| v * speed / length.max(speed));
    if !clear(world, body, [0.0; 3], motion) {
        if route {
            crate::flight_route::blocked(mob.id());
        }
        return false;
    }
    if !route {
        crate::flight_route::forget(mob.id());
    }
    let yaw = if delta[0].hypot(delta[2]) > 0.1 {
        (-delta[0]).atan2(delta[2]).to_degrees() as f32
    } else {
        view.yaw
    };
    mob.reset_fall_distance();
    mob.change(&EntityChange {
        mask: SET_MOTION | SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        motion,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..Default::default()
    })
}

/// Try to descend on dry ground below the dragon; otherwise return to its takeoff site.
/// shortcut: the async landing approach and recovery controller are not exposed.
fn land(world: World<'_>, mob: Entity<'_>, view: EntityView, home: [f64; 3]) {
    let at = view.position;
    let pos = cell(at);
    for i in 1..=96 {
        let p = Pos {
            y: pos.y - i,
            ..pos
        };
        if world.fluid_id(p).as_deref() != Some("minecraft:empty") {
            break;
        }
        let Some(shape) = world.collision_shape(p) else {
            break;
        };
        if !shape.is_empty() {
            if world.face_sturdy(p, 1)
                && steer(
                    world,
                    mob,
                    view,
                    [at[0], f64::from(p.y) + 1.0, at[2]],
                    0.42,
                    false,
                )
            {
                return;
            }
            break;
        }
    }
    let horizontal = (home[0] - at[0]).hypot(home[2] - at[2]);
    let target = [
        home[0],
        if horizontal < 1.0 { home[1] } else { at[1] },
        home[2],
    ];
    if !steer(world, mob, view, target, 0.7, horizontal >= 1.0) {
        // Stop drifting into an obstacle while a safe descent is sought again.
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion: [0.0; 3],
            ..Default::default()
        });
    }
}

pub(super) fn goal(
    phase: u32,
    world: World<'_>,
    mob: Entity<'_>,
    mut view: EntityView,
    state: &mut FlightState,
    now: u64,
) -> bool {
    let (Some(ai), Some(mobs)) = (MOB_AI.get(), MOBS.get()) else {
        return false;
    };
    let Some(weather) = world.weather_state() else {
        return false;
    };
    view.on_ground = grounded(world, mob, view);
    let p = profile(weather.thundering && weather.raining, weather.raining);
    let free =
        mobs.passengers(mob).is_empty() && mob.vehicle().is_none() && ai.target(mob).is_none();
    match phase {
        goal_phase::CAN_START => {
            if !free
                || taming::is_tamed(mob)
                || crate::eggs::is_juvenile(mob)
                || crate::combat_raevyx::storm_casting(mob)
                || (view.on_ground && now.saturating_sub(state.last_landing) < p.landing_cooldown)
                || now < state.retry
            {
                return false;
            }
            state.retry = now + p.decision + random((p.decision / 2).max(1));
            let force = danger(world, view.position);
            let should_fly = force
                || if view.on_ground {
                    random(p.takeoff) == 0
                        && mob
                            .body()
                            .is_some_and(|body| clear(world, body, [0.0; 3], [0.0, 3.0, 0.0]))
                } else {
                    random(p.keep) != 0
                };
            if !should_fly {
                return false;
            }
            state.pending = cruise_target(world, mob, view, p, now, false);
            state.pending.is_some() || !view.on_ground
        }
        goal_phase::START => {
            let cruise = state.pending.take();
            state.active = Some(Flight {
                home: view.position,
                heading: f64::from(view.yaw).to_radians(),
                start: now,
                landing: cruise.is_none(),
                released: false,
                cruise,
            });
            ai.stop(mob);
            false
        }
        goal_phase::CONTINUE => {
            let Some(flight) = state.active.as_mut() else {
                return false;
            };
            if !free || crate::combat_raevyx::storm_casting(mob) {
                return false;
            }
            flight.landing |= taming::is_tamed(mob)
                || (!view.on_ground && random(p.keep) == 0 && !danger(world, view.position));
            !flight.landing || !view.on_ground
        }
        goal_phase::STOP => {
            crate::flight_route::forget(mob.id());
            state.active = None;
            state.pending = None;
            if view.on_ground {
                state.last_landing = now;
            }
            false
        }
        goal_phase::TICK => {
            let Some(flight) = state.active.as_mut() else {
                return false;
            };
            if flight.landing {
                if !view.on_ground {
                    land(world, mob, view, flight.home);
                }
                return false;
            }
            let Some(cruise) = flight.cruise else {
                return false;
            };
            let distance = length_squared(offset(cruise.target, view.position));
            let retarget = distance < REACHED_SQ && !view.on_ground
                || now.saturating_sub(cruise.since) > TARGET_AGE;
            if retarget {
                flight.cruise = cruise_target(world, mob, view, p, now, false);
            }
            let moved = flight.cruise.is_some_and(|cruise| {
                let speed = if cruise.dive {
                    CRUISE_SPEED * 1.6
                } else {
                    CRUISE_SPEED
                };
                steer(world, mob, view, cruise.target, speed, true)
            });
            if !moved {
                mob.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: [0.0; 3],
                    ..Default::default()
                });
                // Keep an airborne destination while its planner searches for a detour.
                if view.on_ground || flight.cruise.is_none() {
                    crate::flight_route::forget(mob.id());
                    flight.cruise = cruise_target(world, mob, view, p, now, true);
                    if flight.cruise.is_none() {
                        flight.landing = true;
                    }
                }
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
    fn ground_support_requires_foot_contact_and_horizontal_overlap() {
        let body = EntityBody {
            min: [0.2, 1.0, 0.2],
            max: [2.2, 3.2, 2.2],
            input: [0.0; 3],
        };
        assert!(supports(body, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
        assert!(!supports(body, [0.0, 0.0, 0.0], [1.0, 0.9, 1.0]));
        assert!(!supports(body, [3.0, 0.0, 0.0], [4.0, 1.0, 1.0]));
        assert!(!supports(body, [0.0, 3.2, 0.0], [1.0, 4.2, 1.0]));
    }

    #[test]
    fn weather_profile_matches_release() {
        let clear = profile(false, false);
        let rain = profile(false, true);
        let thunder = profile(true, true);
        assert_eq!(
            (
                clear.landing_cooldown,
                clear.decision,
                clear.takeoff,
                clear.keep,
                clear.max_height
            ),
            (100, 25, 80, 200, 50.0)
        );
        assert_eq!(
            (rain.decision, rain.takeoff, rain.keep, rain.max_height),
            (25, 80, 200, 70.0)
        );
        assert_eq!(
            (
                thunder.landing_cooldown,
                thunder.decision,
                thunder.takeoff,
                thunder.keep,
                thunder.max_height
            ),
            (0, 2, 4, 3000, 90.0)
        );
    }

    #[test]
    fn dive_pulls_out_above_ground() {
        assert_eq!(dive_height(50.0, 20.0, 80.0), Some(30.0));
        assert_eq!(
            dive_height(50.0, 20.0, 10.0),
            Some(50.0 - 10.0 * 34.0_f64.to_radians().tan())
        );
        assert_eq!(dive_height(50.0, 45.0, 20.0), None);
        assert_eq!(dive_height(50.0, 20.0, 7.0), None);
    }

    #[test]
    fn cruise_height_respects_floor_and_ceiling_margins() {
        let space = Space {
            floor: Some(20.0),
            ceiling: Some(30.0),
        };
        assert_eq!(fit_height(40.0, &space, 3.0), Some(25.5));
        assert_eq!(fit_height(0.0, &space, 3.0), Some(21.5));
        assert_eq!(fit_height(25.0, &space, 8.0), None);
    }
}
