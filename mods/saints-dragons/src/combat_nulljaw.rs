//! Nulljaw bite, shulker hunting and coordinated aerial attack runs (0.9.88).

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{
        ENTITY_ALIVE, Entity, EntityChange, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION, SET_YAW,
    },
    mob_ai::goal_phase,
    tick_count,
};
use serde::{Deserialize, Serialize};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, abilities, flight, hunger, taming};

pub const TARGETING: u64 = 160;
pub const COMBAT: u64 = 161;
const KIND: &str = "saintsdragons:nulljaw";
const WIDTH: f64 = 1.2;
const FOLLOW: f64 = 48.0;
const FLIGHT: f64 = 0.42;
const BITE_STARTUP: u64 = 5;
const BITE_DURATION: u64 = 17;
const BITE_COOLDOWN: u64 = 12;

type Point = [f64; 3];
type GroupKey = (i32, i32);

#[derive(Default)]
struct Brain {
    last_hurt: Option<i32>,
    next_scan: u64,
    group: Option<GroupKey>,
    bite: Option<(u64, i32)>,
    ready: u64,
    eat_until: u64,
    projectile: Option<i32>,
    next_projectile_scan: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Orbit,
    Stage,
    Dive,
    Egress,
}

struct Run {
    phase: Phase,
    start: u64,
    direction: Point,
    bite: Option<u64>,
    exit: Option<Point>,
}

#[derive(Default)]
struct Group {
    members: Vec<i32>,
    member_seen: HashMap<i32, u64>,
    runs: HashMap<i32, Run>,
    next_attack: u64,
    next_slot: usize,
    wave: usize,
    seen: u64,
}

#[derive(Default)]
struct State {
    brains: HashMap<i32, Brain>,
    groups: HashMap<GroupKey, Group>,
}

// Transient combat state, as in the Java coordinator and Cindervane combat.
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    f(STATE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default())
}

fn brain<T>(id: i32, f: impl FnOnce(&mut Brain) -> T) -> T {
    state(|s| f(s.brains.entry(id).or_default()))
}

fn scale(v: Point, n: f64) -> Point {
    v.map(|x| x * n)
}
fn distance2(a: Point, b: Point) -> f64 {
    flight::length_squared(flight::offset(a, b))
}
fn radial(angle: f64) -> Point {
    [angle.cos(), 0.0, angle.sin()]
}

const CLOAK_DATA: &str = "saintsdragons:nulljaw_cloak";
const INVISIBILITY: &str = "minecraft:invisibility";

#[derive(Deserialize, Serialize)]
struct Cloak {
    remaining: i32,
    rider: fastnbt::IntArray,
}

impl Cloak {
    fn load(mob: Entity<'_>) -> Option<Self> {
        let cloak: Self = fastnbt::from_bytes(&mob.data(CLOAK_DATA)?).ok()?;
        (cloak.remaining > 0 && cloak.rider.len() == 4).then_some(cloak)
    }

    fn uuid(&self) -> [u8; 16] {
        let mut uuid = [0; 16];
        for (bytes, value) in uuid
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.rider.iter())
        {
            bytes.copy_from_slice(&value.to_be_bytes());
        }
        uuid
    }
}

/// Nulljaw.deactivateCloak: remove the effect from both participants, even
/// when the recorded rider has already dismounted.
fn deactivate_cloak(world: World<'_>, mob: Entity<'_>, cloak: &Cloak) {
    mob.remove_effect(INVISIBILITY);
    if let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get())
        && let Some(id) = mobs.by_uuid(world, &cloak.uuid())
    {
        entities.with_entity(world, id, |rider| {
            if rider.type_id() == "minecraft:player" {
                rider.remove_effect(INVISIBILITY);
            }
        });
    }
    mob.set_data(CLOAK_DATA, &[]);
}

/// Nulljaw.tickCloak: validate the recorded controlling owner and count loaded ticks.
/// shortcut: the custom secondary ability key is not exposed by the rider input ABI.
pub fn cloak_tick(world: World<'_>, mob: Entity<'_>) {
    let (Some(mut cloak), Some(mobs), Some(entities)) =
        (Cloak::load(mob), MOBS.get(), ENTITIES.get())
    else {
        return;
    };
    let uuid = cloak.uuid();
    let rider = mobs.passengers(mob).first().copied().filter(|&id| {
        entities.with_entity(world, id, |e| {
            e.type_id() == "minecraft:player" && mobs.uuid(e) == Some(uuid)
        }) == Some(true)
    });
    cloak.remaining -= 1;
    if let Some(rider) = rider
        && cloak.remaining > 0
        && taming::owner(mob) == Some(uuid)
    {
        // shortcut: no has-effect query or independent icon flag. Refresh the
        // remaining duration periodically instead of reapplying every tick.
        if cloak.remaining % 20 == 0 {
            mob.add_effect(INVISIBILITY, cloak.remaining, 0, false, false);
            entities.with_entity(world, rider, |e| {
                e.add_effect(INVISIBILITY, cloak.remaining, 0, false, false)
            });
        }
        if let Ok(nbt) = fastnbt::to_bytes(&cloak) {
            mob.set_data(CLOAK_DATA, &nbt);
        }
    } else {
        deactivate_cloak(world, mob, &cloak);
    }
}

pub fn death(world: World<'_>, mob: Entity<'_>) {
    if MOBS.get().and_then(|m| m.kind(mob)) == Some(KIND)
        && let Some(cloak) = Cloak::load(mob)
    {
        deactivate_cloak(world, mob, &cloak);
    }
}

fn eligible(mob: Entity<'_>) -> bool {
    mob.flags() & ENTITY_ALIVE != 0
        && !crate::eggs::is_juvenile(mob)
        && mob.vehicle().is_none()
        && taming::order_name(mob) != "stay"
        && MOBS.get().is_some_and(|m| m.passengers(mob).is_empty())
}

fn enemy(mob: Entity<'_>, other: Entity<'_>) -> bool {
    abilities::enemy(mob, other)
        && MOBS.get().is_some_and(|m| {
            m.kind(other) != Some(KIND)
                && !(taming::owner(mob).is_some() && taming::owner(mob) == taming::owner(other))
        })
}

fn near(world: World<'_>, at: Point, radius: f64, except: i32) -> Vec<i32> {
    ENTITIES.get().map_or_else(Vec::new, |e| {
        e.entities_in(
            world,
            at.map(|v| v - radius),
            at.map(|v| v + radius),
            except,
        )
    })
}

/// Conservative loaded-block ray test (the host has no entity sensing raycast).
fn visible(world: World<'_>, from: Point, to: Point) -> bool {
    let delta = flight::offset(to, from);
    let steps = (flight::length_squared(delta).sqrt() * 4.0).ceil() as usize;
    (1..steps).all(|i| {
        world
            .collision_shape(flight::cell(flight::add(
                from,
                scale(delta, i as f64 / steps as f64),
            )))
            .is_some_and(|shape| shape.is_empty())
    })
}

fn leave(s: &mut State, id: i32) {
    if let Some(key) = s.brains.entry(id).or_default().group.take()
        && let Some(group) = s.groups.get_mut(&key)
    {
        group.members.retain(|&member| member != id);
        group.member_seen.remove(&id);
        if group.runs.remove(&id).is_some() && group.runs.is_empty() {
            group.next_attack = tick_count() + 32 + group.members.len().max(1) as u64 * 5;
        }
    }
}

fn assign(s: &mut State, id: i32, key: GroupKey) {
    if s.brains.entry(id).or_default().group == Some(key) {
        return;
    }
    leave(s, id);
    s.brains.entry(id).or_default().group = Some(key);
    let group = s.groups.entry(key).or_insert_with(|| Group {
        next_attack: tick_count() + 20,
        ..Group::default()
    });
    if !group.members.contains(&id) {
        group.members.push(id);
    }
    group.member_seen.insert(id, tick_count());
}

/// Nulljaw.alertNearbyNulljaws: a wild dragon alerts squads of at most four;
/// a tamed dragon retaliates alone.
fn retaliate(world: World<'_>, mob: Entity<'_>, attacker: i32, at: Point) {
    let (Some(entities), Some(mobs), Some(ai)) = (ENTITIES.get(), MOBS.get(), MOB_AI.get()) else {
        return;
    };
    if !eligible(mob)
        || !entities
            .with_entity(world, attacker, |e| enemy(mob, e))
            .unwrap_or(false)
    {
        return;
    }
    let mut responders = vec![(mobs.uuid(mob).unwrap_or_default(), mob.id())];
    if !taming::is_tamed(mob) {
        for id in near(world, at, 28.0, mob.id()) {
            if let Some(Some(uuid)) = entities.with_entity(world, id, |e| {
                (mobs.kind(e) == Some(KIND)
                    && eligible(e)
                    && !taming::is_tamed(e)
                    && entities
                        .with_entity(world, attacker, |a| enemy(e, a))
                        .unwrap_or(false))
                .then(|| mobs.uuid(e))
                .flatten()
            }) {
                responders.push((uuid, id));
            }
        }
    }
    responders.sort_unstable();
    for squad in responders.chunks(4) {
        // shortcut: UUID order chooses the squad leader; idle pack leadership is not exposed.
        let key = (squad[0].1, attacker);
        for &(_, id) in squad {
            entities.with_entity(world, id, |e| ai.set_target(world, e, Some(attacker)));
            state(|s| assign(s, id, key));
        }
    }
}

/// NulljawTargetingBehaviour: assigned targets, retaliation and visible shulkers.
pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(entities), Some(view)) = (MOB_AI.get(), ENTITIES.get(), mob.view()) else {
        return false;
    };
    if !eligible(mob) {
        ai.set_target(world, mob, None);
        return false;
    }
    if let Some((attacker, timestamp)) = ai.last_hurt_by(mob)
        && brain(mob.id(), |b| {
            b.last_hurt.replace(timestamp) != Some(timestamp)
        })
    {
        retaliate(world, mob, attacker, view.position);
    }
    if let Some(id) = ai.target(mob) {
        if entities
            .with_entity(world, id, |e| {
                enemy(mob, e)
                    && e.view()
                        .is_some_and(|v| distance2(v.position, view.position) <= FOLLOW * FOLLOW)
            })
            .unwrap_or(false)
        {
            return false;
        }
        ai.set_target(world, mob, None);
        state(|s| leave(s, mob.id()));
    }
    let now = tick_count();
    if !brain(mob.id(), |b| {
        if now < b.next_scan {
            return false;
        }
        b.next_scan = now + 11;
        true
    }) {
        return false;
    }
    let eye = flight::add(view.position, [0.0, 0.6, 0.0]);
    let nearest = near(world, view.position, 16.0, mob.id())
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |e| {
                    let v = e.view()?;
                    let d = distance2(v.position, view.position);
                    (e.type_id().trim_start_matches("minecraft:") == "shulker"
                        && enemy(mob, e)
                        && d <= 256.0
                        && visible(world, eye, flight::add(v.position, [0.0, 0.5, 0.0])))
                    .then_some((id, d))
                })
                .flatten()
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((id, _)) = nearest {
        ai.set_target(world, mob, Some(id));
    }
    false
}

impl Group {
    fn directive(
        &mut self,
        id: i32,
        now: u64,
        at: Point,
        center: Point,
        angle: f64,
    ) -> (Phase, Point, bool) {
        self.seen = now;
        let count = self.members.len().max(1);
        let slot = self.members.iter().position(|&m| m == id).unwrap_or(0);
        let slot_angle = |n: usize| angle + n as f64 * std::f64::consts::TAU / count as f64;
        if self.runs.is_empty() && now >= self.next_attack {
            let primary = self.next_slot % count;
            self.next_slot += 1;
            let mut launch = |n: usize, delay: u64| {
                if let Some(&member) = self.members.get(n) {
                    self.runs.insert(
                        member,
                        Run {
                            phase: Phase::Stage,
                            start: now + delay,
                            direction: radial(slot_angle(n)),
                            bite: None,
                            exit: None,
                        },
                    );
                }
            };
            launch(primary, 0);
            if count >= 3 && self.wave.is_multiple_of(2) {
                launch((primary + count / 2) % count, 6);
            }
            self.wave += 1;
        }
        if let Some(run) = self.runs.get_mut(&id)
            && now >= run.start
        {
            if run.phase == Phase::Stage {
                let stage = flight::add(
                    flight::add(center, scale(run.direction, 10.0)),
                    [0.0, 3.5 + (slot % 2) as f64 * 2.0, 0.0],
                );
                if distance2(at, stage) <= 9.0 || now - run.start >= 55 {
                    run.phase = Phase::Dive;
                    run.start = now;
                } else {
                    return (Phase::Stage, stage, false);
                }
            }
            if run.phase == Phase::Dive {
                if run.bite.is_some_and(|t| now - t >= 7)
                    || (run.bite.is_none() && now - run.start >= 35)
                {
                    run.phase = Phase::Egress;
                    run.start = now;
                    run.exit = Some(flight::add(
                        flight::add(center, scale(run.direction, -11.0)),
                        [0.0, 3.0, 0.0],
                    ));
                } else {
                    return (Phase::Dive, center, run.bite.is_none());
                }
            }
            if let Some(exit) = run.exit
                && distance2(at, exit) > 12.25
                && now - run.start < 55
            {
                return (Phase::Egress, exit, false);
            }
            self.runs.remove(&id);
            if self.runs.is_empty() {
                self.next_attack = now + 32 + count as u64 * 5;
            }
        }
        let height =
            [2.5, 5.0, 3.75, 6.0][slot % 4] + (now as f64 * 0.08 + slot as f64 * 1.7).sin() * 0.45;
        let point = flight::add(
            flight::add(
                center,
                scale(radial(slot_angle(slot)), 8.5 + (slot % 2) as f64 * 1.25),
            ),
            [0.0, height, 0.0],
        );
        (Phase::Orbit, point, false)
    }
}

/// NulljawTacticalCombatBehaviour intercept-time quadratic.
fn intercept_time(relative: Point, velocity: Point, speed: f64) -> f64 {
    let a = flight::length_squared(velocity) - speed * speed;
    let b = 2.0
        * relative
            .iter()
            .zip(velocity)
            .map(|(x, y)| x * y)
            .sum::<f64>();
    let c = flight::length_squared(relative);
    let fallback = c.sqrt() / speed;
    if a.abs() < 1.0e-6 {
        return if b.abs() > 1.0e-6 && -c / b > 0.0 {
            -c / b
        } else {
            fallback
        };
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return fallback;
    }
    [
        (-b - disc.sqrt()) / (2.0 * a),
        (-b + disc.sqrt()) / (2.0 * a),
    ]
    .into_iter()
    .filter(|t| *t > 0.0)
    .min_by(f64::total_cmp)
    .unwrap_or(fallback)
}

fn projectile(world: World<'_>, mob: Entity<'_>, at: Point, now: u64) -> Option<i32> {
    let (entities, mobs) = (ENTITIES.get()?, MOBS.get()?);
    let owner = mobs.by_uuid(world, &taming::owner(mob)?)?;
    let (owner_at, owner_body) = entities.with_entity(world, owner, |e| {
        (e.flags() & ENTITY_ALIVE != 0).then_some((e.view()?.position, e.body()?))
    })??;
    if distance2(at, owner_at) > 48.0 * 48.0 {
        return None;
    }
    let previous = brain(mob.id(), |b| b.projectile);
    let valid = |id| {
        entities
            .with_entity(world, id, |e| {
                e.type_id().trim_start_matches("minecraft:") == "shulker_bullet"
                    && e.flags() & ENTITY_ALIVE != 0
                    && e.projectile_owner().is_some_and(|shooter| {
                        entities.with_entity(world, shooter, |s| {
                            s.type_id().trim_start_matches("minecraft:") == "shulker"
                        }) == Some(true)
                    })
                    && e.view()
                        .is_some_and(|v| distance2(v.position, owner_at) <= 1600.0)
            })
            .unwrap_or(false)
    };
    if let Some(id) = previous.filter(|&id| valid(id)) {
        return Some(id);
    }
    if !brain(mob.id(), |b| {
        b.projectile = None;
        if now < b.next_projectile_scan {
            return false;
        }
        b.next_projectile_scan = now + 5;
        true
    }) {
        return None;
    }
    let owner_center = scale(flight::add(owner_body.min, owner_body.max), 0.5);
    let selected = entities
        .entities_matching(
            world,
            owner_body.min.map(|v| v - 32.0),
            owner_body.max.map(|v| v + 32.0),
            mob.id(),
            0,
        )
        .into_iter()
        .filter_map(|id| {
            if !valid(id) {
                return None;
            }
            entities
                .with_entity(world, id, |e| {
                    let v = e.view()?;
                    let targeted = e.projectile_owner().is_some_and(|shooter| {
                        entities.with_entity(world, shooter, |s| {
                            MOB_AI.get().and_then(|ai| ai.target(s)) == Some(owner)
                        }) == Some(true)
                    });
                    let delta = flight::offset(owner_center, v.position);
                    (targeted || approaching_owner(delta, v.motion))
                        .then_some((id, distance2(owner_at, v.position)))
                })
                .flatten()
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id);
    brain(mob.id(), |b| b.projectile = selected);
    selected
}

fn approaching_owner(delta: Point, motion: Point) -> bool {
    flight::length_squared(delta) <= 64.0
        && delta.iter().zip(motion).map(|(x, y)| x * y).sum::<f64>() > 0.0
}

fn sound(world: World<'_>, mob: Entity<'_>, original: &str, fallback: &str) {
    if let Some(effects) = EFFECTS.get() {
        let sound = if cfg!(feature = "original-assets") {
            original
        } else {
            fallback
        };
        effects.play_entity_sound(world, mob.id(), sound, SOUND_NEUTRAL, 1.0, 1.0);
    }
}

/// Direct air steering, with a collision check. shortcut: strict-air A* needs a host
/// flying navigation interface; use an upward detour when the direct route is blocked.
fn fly(world: World<'_>, mob: Entity<'_>, at: Point, waypoint: Point, speed: f64) {
    let Some(body) = mob.body() else { return };
    let delta = flight::offset(waypoint, at);
    let length = flight::length_squared(delta).sqrt();
    let mut motion = scale(delta, speed / length.max(speed));
    if !flight::clear_body(world, body, motion) {
        motion = [0.0, speed, 0.0];
        if !flight::clear_body(world, body, motion) {
            motion = [0.0; 3];
        }
    }
    let yaw = if delta[0].hypot(delta[2]) > 0.01 {
        delta[2].atan2(delta[0]).to_degrees() as f32 - 90.0
    } else {
        mob.view().map_or(0.0, |v| v.yaw)
    };
    mob.change(&EntityChange {
        mask: SET_MOTION | SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        motion,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..EntityChange::default()
    });
    mob.reset_fall_distance();
}

fn bite_tick(world: World<'_>, mob: Entity<'_>, now: u64) {
    let active = brain(mob.id(), |b| b.bite);
    let Some((start, id)) = active else { return };
    if now >= start + BITE_DURATION {
        brain(mob.id(), |b| {
            b.bite = None;
            b.ready = start + BITE_DURATION + BITE_COOLDOWN;
        });
    } else if now == start + BITE_STARTUP {
        let (Some(ai), Some(entities), Some(view)) = (MOB_AI.get(), ENTITIES.get(), mob.view())
        else {
            return;
        };
        let damage = 8.0 * hunger::melee_multiplier(mob);
        let valid = entities
            .with_entity(world, id, |e| {
                let (Some(v), Some(body)) = (e.view(), e.body()) else {
                    return false;
                };
                enemy(mob, e)
                    && distance2(v.position, view.position)
                        <= reach(2.75, body.max[0] - body.min[0]).powi(2)
            })
            .unwrap_or(false);
        if valid && ai.attack(world, mob, id, damage) {
            entities.with_entity(world, id, |e| {
                if let Some(v) = e.view() {
                    let yaw = f64::from(view.yaw).to_radians();
                    e.change(&EntityChange {
                        mask: SET_MOTION,
                        motion: flight::add(v.motion, [-yaw.sin() * 0.2, 0.05, yaw.cos() * 0.2]),
                        ..EntityChange::default()
                    });
                }
            });
        }
    }
}

fn reach(extra: f64, width: f64) -> f64 {
    extra + (WIDTH + width) * 0.5
}

pub fn clip(mob: Entity<'_>) -> Option<&'static str> {
    brain(mob.id(), |b| {
        if b.bite.is_some() {
            Some("bite")
        } else if tick_count() < b.eat_until {
            Some("eat")
        } else {
            None
        }
    })
}

pub fn combat(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return false;
    };
    let now = tick_count();
    let can = eligible(mob);
    let bullet = if can
        && matches!(
            phase,
            goal_phase::CAN_START | goal_phase::CONTINUE | goal_phase::TICK
        ) {
        projectile(world, mob, view.position, now)
    } else {
        None
    };
    let target = ai.target(mob).and_then(|id| {
        entities
            .with_entity(world, id, |e| {
                let (v, body) = (e.view()?, e.body()?);
                (enemy(mob, e) && distance2(v.position, view.position) <= FOLLOW * FOLLOW)
                    .then_some((id, v, body))
            })
            .flatten()
    });
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => {
            can && (bullet.is_some() || target.is_some())
        }
        goal_phase::START => {
            ai.stop(mob);
            mobs.set_no_gravity(mob, true);
            false
        }
        goal_phase::STOP => {
            state(|s| {
                leave(s, mob.id());
                s.brains.entry(mob.id()).or_default().bite = None;
            });
            ai.stop(mob);
            mobs.set_no_gravity(mob, false);
            false
        }
        goal_phase::TICK => {
            bite_tick(world, mob, now);
            if let Some(id) = bullet {
                state(|s| leave(s, mob.id()));
                if let Some(Some((v, body))) =
                    entities.with_entity(world, id, |e| Some((e.view()?, e.body()?)))
                {
                    let center = scale(flight::add(body.min, body.max), 0.5);
                    if let Some(own) = mob.body()
                        && (0..3).all(|i| {
                            own.min[i] - 1.25 <= body.max[i] && own.max[i] + 1.25 >= body.min[i]
                        })
                    {
                        entities.with_entity(world, id, |e| e.discard());
                        brain(mob.id(), |b| {
                            b.projectile = None;
                            b.eat_until = now + 22;
                        });
                        sound(
                            world,
                            mob,
                            "saintsdragons:nulljaw_eat",
                            "entity.generic.eat",
                        );
                    } else {
                        let time = intercept_time(
                            flight::offset(center, view.position),
                            v.motion,
                            FLIGHT * 1.35,
                        )
                        .clamp(1.5, 10.0);
                        fly(
                            world,
                            mob,
                            view.position,
                            flight::add(center, scale(v.motion, time)),
                            FLIGHT * 1.35,
                        );
                    }
                }
                return false;
            }
            let Some((id, v, body)) = target else {
                return false;
            };
            let center = flight::add(
                scale(flight::add(body.min, body.max), 0.5),
                scale(v.motion, 2.0),
            );
            let reverse = entities
                .with_entity(world, id, |e| mobs.uuid(e))
                .flatten()
                .is_some_and(|uuid| uuid[15] & 1 != 0);
            let (key, mode, waypoint, may_bite) = state(|s| {
                s.groups
                    .retain(|_, g| now.saturating_sub(g.seen) <= 200 || g.seen == 0);
                let key = s
                    .brains
                    .entry(mob.id())
                    .or_default()
                    .group
                    .filter(|k| k.1 == id)
                    .unwrap_or((mob.id(), id));
                if !s.groups.contains_key(&key) {
                    s.brains.entry(mob.id()).or_default().group = None;
                }
                assign(s, mob.id(), key);
                let group = s.groups.get_mut(&key).expect("assigned group");
                group.member_seen.insert(mob.id(), now);
                // Members whose goals stopped ticking (unloaded or dead) cannot reserve a wave.
                group.members.retain(|id| {
                    group
                        .member_seen
                        .get(id)
                        .is_some_and(|seen| now.saturating_sub(*seen) <= 20)
                });
                group.runs.retain(|id, _| group.members.contains(id));
                if !group.members.contains(&mob.id()) {
                    group.members.push(mob.id());
                }
                if group.members.len() == 1 && group.seen == 0 {
                    group.next_attack = now;
                }
                let angle = now as f64 * 0.035 * if reverse { -1.0 } else { 1.0 };
                let (mode, waypoint, may_bite) =
                    group.directive(mob.id(), now, view.position, center, angle);
                (key, mode, waypoint, may_bite)
            });
            if may_bite
                && distance2(view.position, v.position)
                    <= reach(1.5, body.max[0] - body.min[0]).powi(2)
                && visible(world, flight::add(view.position, [0.0, 0.6, 0.0]), center)
                && brain(mob.id(), |b| {
                    if b.bite.is_some() || now < b.ready {
                        return false;
                    }
                    b.bite = Some((now, id));
                    true
                })
            {
                state(|s| {
                    if let Some(run) = s
                        .groups
                        .get_mut(&key)
                        .and_then(|g| g.runs.get_mut(&mob.id()))
                    {
                        run.bite = Some(now);
                    }
                });
                sound(
                    world,
                    mob,
                    "saintsdragons:nulljaw_bite",
                    "entity.spider.hurt",
                );
            }
            let speed = match mode {
                Phase::Orbit => 0.9,
                Phase::Stage => 1.1,
                Phase::Dive => 1.4,
                Phase::Egress => 1.3,
            };
            fly(world, mob, view.position, waypoint, FLIGHT * speed);
            ai.look_at(mob, center);
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn last_chance_requires_close_approaching_bullet() {
        assert!(approaching_owner([8.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        assert!(!approaching_owner([8.1, 0.0, 0.0], [1.0, 0.0, 0.0]));
        assert!(!approaching_owner([1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]));
    }

    #[test]
    fn cloak_uuid_matches_vanilla_int_array() {
        let cloak = Cloak {
            remaining: 6000,
            rider: fastnbt::IntArray::new(vec![0x01020304, 0x05060708, -1, 0]),
        };
        let loaded: Cloak = fastnbt::from_bytes(&fastnbt::to_bytes(&cloak).unwrap()).unwrap();
        assert_eq!(loaded.remaining, 6000);
        assert_eq!(
            loaded.uuid(),
            [1, 2, 3, 4, 5, 6, 7, 8, 255, 255, 255, 255, 0, 0, 0, 0]
        );
    }
    #[test]
    fn bite_and_intercept_geometry() {
        assert_eq!(reach(2.75, 1.2), 3.95);
        assert_eq!(reach(1.5, 1.2), 2.7);
        assert!((intercept_time([10.0, 0.0, 0.0], [-1.0, 0.0, 0.0], 1.0) - 5.0).abs() < 1e-8);
        assert_eq!(intercept_time([10.0, 0.0, 0.0], [0.0; 3], 2.0), 5.0);
    }
    #[test]
    fn pincer_is_staggered_and_committed_bite_exits() {
        let mut group = Group {
            members: vec![1, 2, 3, 4],
            ..Group::default()
        };
        let center = [0.0; 3];
        assert_eq!(
            group.directive(1, 10, [10.0, 3.5, 0.0], center, 0.0).0,
            Phase::Dive
        );
        assert_eq!(
            group.directive(3, 15, [-10.0, 3.5, 0.0], center, 0.0).0,
            Phase::Orbit
        );
        assert_eq!(
            group.directive(3, 16, [-10.0, 3.5, 0.0], center, 0.0).0,
            Phase::Dive
        );
        group.runs.get_mut(&1).unwrap().bite = Some(11);
        assert_eq!(group.directive(1, 18, center, center, 0.0).0, Phase::Egress);
    }
}
