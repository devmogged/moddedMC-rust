//! Ignivorus ground combat and Skyfall phase transition from 0.9.88.

use crate::combat_raevyx::learning;
use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, abilities, taming};
use moddedmc_api::{
    abi::SOUND_NEUTRAL,
    content::World,
    entities::{Entity, EntityChange, SET_BODY_YAW, SET_MOTION, SET_YAW},
    mob_ai::goal_phase,
    mobs::ProjectileStep,
    tick_count,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const TARGETING: u64 = 110;
pub const COMBAT: u64 = 111;
const DATA: &str = "saintsdragons:ignivorus_combat";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Attack {
    Bite,
    Slam,
    Stomp,
    Wing,
    Breath,
    Roar,
    Fireball,
    Skyfall,
    Leap,
}

impl Attack {
    fn clip(self) -> &'static str {
        match self {
            Self::Bite => "bite",
            Self::Slam => "body_slam",
            Self::Stomp => "stomp_right",
            Self::Wing => "wing_swipe_right",
            Self::Breath => "fire_breathing",
            Self::Roar => "roar",
            Self::Fireball => "fireball_level1_charge",
            Self::Skyfall => "skyfall",
            Self::Leap => "leap",
        }
    }
    fn duration(self, charge: i32) -> i32 {
        match self {
            Self::Bite => 17,
            Self::Slam => 30,
            Self::Stomp => 29,
            Self::Wing => 25,
            Self::Breath => 249,
            Self::Roar => 60,
            Self::Fireball => charge + 15,
            Self::Skyfall => 290,
            Self::Leap => 48,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
    fn cooldown(self) -> i32 {
        match self {
            Self::Bite | Self::Wing => 30,
            Self::Slam | Self::Stomp => 35,
            Self::Breath => 400,
            Self::Roar => 320,
            Self::Fireball => 200,
            Self::Skyfall => 6000,
            Self::Leap => 140,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    phase_two: bool,
    ultimate_used: bool,
    attack: Option<Attack>,
    age: i32,
    target: i32,
    last_hurt: Option<i32>,
    last: Option<Attack>,
    previous: Option<Attack>,
    ready: [i64; 9],
    next_decision: i64,
    charge: i32,
    right: bool,
    hit: Vec<i32>,
    shots: Vec<Shot>,
    rider: Option<i32>,
    primary_since: Option<i64>,
    releasing: bool,
    breath_used: f32,
    breath_depleted: bool,
    melee_mode: bool,
    flex_until: i64,
    flex_ready: i64,
    phase_ready: i64,
    locked_until: i64,
    phase_animation: Option<bool>,
    bulldoze: bool,
    bulldoze_ready: i64,
    bulldoze_hits: Vec<BulldozeHit>,
    descend: bool,
    ascend: bool,
    accelerate: bool,
    skyfall_offset: i32,
    skyfall_air: bool,
}
#[derive(Serialize, Deserialize)]
struct Shot {
    #[serde(default)]
    id: Option<i32>,
    position: [f64; 3],
    velocity: [f64; 3],
    age: i32,
    multiplier: f32,
    #[serde(default)]
    travelled: f64,
}
#[derive(Serialize, Deserialize)]
struct BulldozeHit {
    id: i32,
    until: i64,
}
impl State {
    fn load(mob: Entity<'_>) -> Self {
        mob.data(DATA)
            .and_then(|v| fastnbt::from_bytes(&v).ok())
            .unwrap_or_default()
    }
    fn save(&self, mob: Entity<'_>) {
        if let Ok(nbt) = fastnbt::to_bytes(self) {
            mob.set_data(DATA, &nbt);
        }
    }
}

fn now() -> i64 {
    tick_count() as i64
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|v| v * k)
}
fn length(a: [f64; 3]) -> f64 {
    a.iter().map(|v| v * v).sum::<f64>().sqrt()
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    scale(a, 1.0 / length(a).max(0.00001))
}
fn forward(yaw: f32) -> [f64; 3] {
    let angle = f64::from(yaw).to_radians();
    [-angle.sin(), 0.0, angle.cos()]
}
fn look_direction(yaw: f32, pitch: f32) -> [f64; 3] {
    let p = f64::from(pitch).to_radians();
    let f = forward(yaw);
    [f[0] * p.cos(), -p.sin(), f[2] * p.cos()]
}

fn rider_aim(world: World<'_>, s: &State) -> Option<[f64; 3]> {
    ENTITIES.get()?.with_entity(world, s.rider?, |rider| {
        rider.view().map(|v| look_direction(v.yaw, v.pitch))
    })?
}
fn target_at(world: World<'_>, mob: Entity<'_>, id: i32) -> Option<([f64; 3], f64)> {
    ENTITIES.get()?.with_entity(world, id, |e| {
        if !abilities::enemy(world, mob, e) {
            return None;
        }
        let (view, body) = (e.view()?, e.body()?);
        Some((view.position, body.max[0] - body.min[0]))
    })?
}
fn eligible(mob: Entity<'_>) -> bool {
    !crate::eggs::is_juvenile(mob)
        && taming::order_name(mob) != "stay"
        && MOBS.get().is_some_and(|m| m.passengers(mob).is_empty())
}

pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(view)) = (MOB_AI.get(), mob.view()) else {
        return false;
    };
    let mut s = State::load(mob);
    if !eligible(mob) {
        ai.set_target(world, mob, None);
        return false;
    }
    if ai.target(mob).is_some_and(|id| {
        target_at(world, mob, id).is_none_or(|(at, _)| length(sub(at, view.position)) > 128.0)
    }) {
        ai.set_target(world, mob, None);
    }
    let valid = |id| {
        target_at(world, mob, id).is_some_and(|(at, _)| length(sub(at, view.position)) <= 128.0)
    };
    if let Some(id) = crate::senses::owner_target(world, mob, true, valid)
        .or_else(|| crate::senses::owner_attack(world, mob, valid))
    {
        ai.set_target(world, mob, Some(id));
        return false;
    }
    if let Some((attacker, stamp)) = ai.last_hurt_by(mob)
        && s.last_hurt != Some(stamp)
    {
        s.last_hurt = Some(stamp);
        if target_at(world, mob, attacker).is_some() {
            ai.set_target(world, mob, Some(attacker));
        }
        s.save(mob);
    }
    false
}

/// GroundCombatBehaviour.selectAbility: base suitability and repetition penalties.
fn choose(
    s: &State,
    gap: f64,
    speed: f64,
    facing: f64,
    time: i64,
    breath_roll: bool,
    weights: [f64; 2],
) -> Option<Attack> {
    let mut best = None;
    let mut high = f64::NEG_INFINITY;
    let mut consider = |attack: Attack, score: f64, valid: bool| {
        if !valid || score < 50.0 || s.ready[attack.index()] > time {
            return;
        }
        let score = (score
            - if s.last == Some(attack) {
                28.0
            } else if s.previous == Some(attack) {
                12.0
            } else {
                0.0
            })
            * match attack {
                Attack::Breath => weights[0],
                Attack::Fireball => weights[1],
                _ => 1.0,
            };
        if score > high {
            high = score;
            best = Some(attack);
        }
    };
    consider(
        Attack::Bite,
        66.0 + gap * 2.0 - if gap <= 2.5 { 12.0 } else { 0.0 },
        !s.phase_two && gap <= 6.0,
    );
    consider(
        Attack::Slam,
        82.0 + (2.5 - gap) * 6.0,
        !s.phase_two && gap <= 2.5,
    );
    consider(
        Attack::Stomp,
        48.0 + if gap <= 4.0 {
            30.0
        } else if (7.0..=18.0).contains(&gap) {
            16.0
        } else {
            0.0
        },
        s.phase_two && gap <= 18.0,
    );
    consider(
        Attack::Wing,
        50.0 + if gap <= 6.0 { 22.0 } else { 0.0 }
            + if (5.0..=20.0).contains(&gap) {
                10.0
            } else {
                0.0
            }
            + if facing < 0.75 { 12.0 } else { 0.0 },
        s.phase_two && gap <= 22.0,
    );
    consider(
        Attack::Fireball,
        54.0 + ((gap - 12.0) * 0.9).clamp(0.0, 24.0) + if speed < 0.22 { 10.0 } else { 0.0 },
        s.phase_two && (8.0..=48.0).contains(&gap),
    );
    consider(
        Attack::Leap,
        58.0 + ((gap - 24.0) * 1.3).clamp(0.0, 30.0),
        s.phase_two && (24.0..=56.0).contains(&gap),
    );
    consider(
        Attack::Breath,
        52.0 + if (10.0..=20.0).contains(&gap) {
            14.0
        } else {
            0.0
        } + if speed < 0.18 { 8.0 } else { 0.0 },
        breath_roll && (10.0..=24.0).contains(&gap),
    );
    consider(
        Attack::Roar,
        52.0 + ((gap - 16.0) * 0.75).clamp(0.0, 20.0)
            + if speed < 0.25 { 10.0 } else { 0.0 }
            + facing * 8.0,
        (16.0..=48.0).contains(&gap) && facing >= 0.45,
    );
    best
}

fn sound(world: World<'_>, at: [f64; 3], clip: &str) {
    if let Some(effects) = EFFECTS.get() {
        let id = if cfg!(feature = "original-assets") {
            format!("saintsdragons:ignivorus_{clip}")
        } else {
            "minecraft:entity.ender_dragon.growl".into()
        };
        effects.play_sound(world, at, &id, SOUND_NEUTRAL, 1.0, 1.0);
    }
}
fn particles(world: World<'_>, at: [f64; 3], radius: f64, kind: &str) {
    if let Some(effects) = EFFECTS.get() {
        effects.particles(world, kind, at, [radius, 0.5, radius], 0.03, 12);
    }
}
#[derive(Clone, Copy)]
struct Hit {
    damage: f32,
    fire: u32,
    push: f64,
    lift: f64,
    melee: bool,
    learn: bool,
}
fn strike(world: World<'_>, mob: Entity<'_>, id: i32, center: [f64; 3], hit: Hit) -> bool {
    let (Some(ai), Some(entities), Some(mobs)) = (MOB_AI.get(), ENTITIES.get(), MOBS.get()) else {
        return false;
    };
    entities
        .with_entity(world, id, |e| {
            if !abilities::enemy(world, mob, e) {
                return false;
            }
            if !hit.melee
                && hit.fire > 0
                && crate::TAGS.get().is_some_and(|tags| {
                    let kind = mobs.kind(e).unwrap_or_else(|| e.type_id());
                    tags.contains("entity_type", "saintsdragons:immune_to/fire", kind)
                })
            {
                return false;
            }
            let Some(view) = e.view() else {
                return false;
            };
            let amount = hit.damage
                * if hit.melee {
                    crate::hunger::melee_multiplier(mob)
                } else {
                    1.0
                };
            let damaged = ai.attack(world, mob, id, amount);
            if hit.learn
                && let Some(uuid) = mobs.uuid(e)
            {
                learning::contact(mob, uuid, damaged);
            }
            if hit.fire > 0 {
                mobs.set_on_fire(e, hit.fire);
            }
            let dir = unit(sub(view.position, center));
            e.change(&EntityChange {
                mask: SET_MOTION,
                motion: add(
                    view.motion,
                    [dir[0] * hit.push, hit.lift, dir[2] * hit.push],
                ),
                ..Default::default()
            });
            true
        })
        .unwrap_or(false)
}
fn area(
    world: World<'_>,
    mob: Entity<'_>,
    center: [f64; 3],
    radius: f64,
    height: f64,
    hit: Hit,
    seen: &mut HashSet<i32>,
) {
    let Some(entities) = ENTITIES.get() else {
        return;
    };
    for id in entities.entities_in(
        world,
        add(center, [-radius, -height, -radius]),
        add(center, [radius, height, radius]),
        mob.id(),
    ) {
        if seen.contains(&id) {
            continue;
        }
        if target_at(world, mob, id).is_some_and(|(at, _)| {
            let d = sub(at, center);
            if radius == height {
                length(d) <= radius
            } else {
                d[0].hypot(d[2]) <= radius && d[1].abs() <= height
            }
        }) && strike(world, mob, id, center, hit)
        {
            seen.insert(id);
        }
    }
}

pub fn clip(mob: Entity<'_>) -> Option<&'static str> {
    let s = State::load(mob);
    if now() < s.flex_until {
        return Some("flex");
    }
    if now() < s.locked_until
        && let Some(enter) = s.phase_animation
    {
        return Some(if enter { "phase2_enter" } else { "phase2_exit" });
    }
    s.attack.map(|a| match a {
        Attack::Breath if s.age < 9 => "fire_breath_start",
        Attack::Fireball if s.rider.is_some() && s.releasing => match charge_multiplier(s.charge) {
            2.0 => "fireball_level3_shoot",
            1.5 => "fireball_level2_shoot",
            _ => "fireball_level1_shoot",
        },
        Attack::Skyfall if s.skyfall_air => "skyfall_air",
        Attack::Skyfall if s.skyfall_offset != 0 => "skyfall_phase2",
        Attack::Fireball if s.charge >= 50 => "fireball_level3_charge",
        Attack::Fireball if s.charge >= 25 => "fireball_level2_charge",
        Attack::Wing if !s.right => "wing_swipe_left",
        Attack::Stomp if !s.right => "stomp_left",
        _ => a.clip(),
    })
}
pub fn immune(mob: Entity<'_>, damage_type: &str) -> bool {
    matches!(
        damage_type,
        "minecraft:in_fire" | "minecraft:on_fire" | "minecraft:lava" | "minecraft:hot_floor"
    ) || (!matches!(
        damage_type,
        "minecraft:out_of_world" | "minecraft:generic_kill"
    ) && State::load(mob).attack == Some(Attack::Skyfall))
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(view)) = (MOB_AI.get(), mob.view()) else {
        return false;
    };
    let mut s = State::load(mob);
    let target = ai
        .target(mob)
        .and_then(|id| target_at(world, mob, id).map(|at| (id, at)));
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => {
            return eligible(mob) && (target.is_some() || s.attack == Some(Attack::Skyfall));
        }
        goal_phase::STOP => {
            if s.rider.is_some() {
                return false;
            }
            s.attack = None;
            learning::finish(mob, learning::Outcome::Cancelled);
            if !eligible(mob) {
                learning::clear(mob);
            } else {
                learning::observe(mob, None, !view.on_ground);
            }
            s.phase_two = false;
            s.ultimate_used = false;
            s.save(mob);
            ai.stop(mob);
        }
        goal_phase::TICK => {
            let observation = target
                .and_then(|(id, _)| {
                    ENTITIES
                        .get()?
                        .with_entity(world, id, learning::Observation::from_entity)
                        .flatten()
                })
                .filter(|o| {
                    world
                        .raycast(
                            add(view.position, [0.0, 3.0, 0.0]),
                            o.center,
                            Some(mob.id()),
                        )
                        .is_some_and(|(block, _)| block.is_none())
                });
            let expectation = learning::observe(mob, observation, !view.on_ground);
            if let Some(attack) = s.attack {
                if matches!(attack, Attack::Breath | Attack::Fireball)
                    && target.is_none_or(|(id, _)| id != s.target)
                {
                    learning::finish(mob, learning::Outcome::Cancelled);
                    s.attack = None;
                    s.save(mob);
                    return false;
                }
                ai.stop(mob);
                s.age += 1;
                tick_attack(world, mob, &mut s, attack, view.position, view.yaw);
                if s.age >= attack.duration(s.charge) {
                    if attack == Attack::Breath {
                        learning::finish(mob, learning::Outcome::Completed);
                    } else if attack == Attack::Fireball {
                        learning::finish(mob, learning::Outcome::Cancelled);
                    }
                    s.attack = None;
                    s.ready[attack.index()] = now() + i64::from(attack.cooldown());
                    s.hit.clear();
                }
                s.save(mob);
                return false;
            }
            let Some((id, (at, width))) = target else {
                return false;
            };
            let delta = sub(at, view.position);
            let own_width = mob.body().map_or(2.5, |b| b.max[0] - b.min[0]);
            let gap = (length(delta) - (own_width + width) * 0.5).max(0.0);
            ai.look_at(mob, at);
            let yaw = delta[2].atan2(delta[0]).to_degrees() as f32 - 90.0;
            mob.change(&EntityChange {
                mask: SET_YAW | SET_BODY_YAW,
                yaw,
                body_yaw: yaw,
                ..Default::default()
            });
            let breath_ready = observation.is_some() && s.ready[Attack::Breath.index()] <= now();
            let fireball_ready =
                observation.is_some() && s.phase_two && s.ready[Attack::Fireball.index()] <= now();
            let stop_gap = if breath_ready || fireball_ready {
                22.0 + if breath_ready {
                    expectation.spacing() * 0.75
                } else {
                    0.0
                }
            } else {
                6.0
            };
            if gap > stop_gap {
                if now() % 5 == 0 || !ai.navigating(mob) {
                    ai.walk_to(mob, at, 1.75);
                }
            } else {
                ai.stop(mob);
            }
            if now() < s.next_decision {
                return false;
            }
            s.next_decision = now() + 8;
            let speed = ENTITIES
                .get()
                .and_then(|e| e.with_entity(world, id, |t| t.view().map(|v| length(v.motion))))
                .flatten()
                .unwrap_or(0.0);
            let ultimate = !s.ultimate_used
                && !taming::is_tamed(mob)
                && MOBS
                    .get()
                    .and_then(|m| m.health(mob))
                    .is_some_and(|(hp, max)| hp <= max * 0.6);
            if let Some(attack) = if ultimate {
                Some(Attack::Skyfall)
            } else {
                observation.and_then(|_| {
                    choose(
                        &s,
                        gap,
                        speed,
                        1.0,
                        now(),
                        crate::ai::random(100) < 65,
                        [
                            expectation.weight(),
                            learning::projectile_expectation(mob, !view.on_ground).weight(),
                        ],
                    )
                })
            } {
                s.previous = s.last;
                s.last = Some(attack);
                s.attack = Some(attack);
                s.target = id;
                s.age = 0;
                s.hit.clear();
                if attack == Attack::Breath {
                    learning::begin(mob, view.position, !view.on_ground);
                } else if attack == Attack::Fireball {
                    learning::begin_projectile(mob, view.position, !view.on_ground);
                }
                s.right = !s.right;
                s.charge = if gap >= 30.0 && speed < 0.22 {
                    50
                } else if gap >= 18.0 {
                    25
                } else {
                    3
                };
                if attack == Attack::Skyfall {
                    s.ultimate_used = true;
                }
                let cue = match attack {
                    Attack::Breath => "fire_breath_start",
                    Attack::Fireball => "level1_charge",
                    Attack::Wing => "wing_swipe",
                    Attack::Stomp => "stomp",
                    _ => attack.clip(),
                };
                sound(world, view.position, cue);
                ai.stop(mob);
            }
            s.save(mob);
        }
        _ => {}
    }
    false
}

fn tick_attack(
    world: World<'_>,
    mob: Entity<'_>,
    s: &mut State,
    a: Attack,
    at: [f64; 3],
    yaw: f32,
) {
    let age = s.age;
    let mut seen = HashSet::new();
    let f = forward(yaw);
    let melee = |damage, fire, push, lift| Hit {
        damage,
        fire,
        push,
        lift,
        melee: true,
        learn: false,
    };
    let ranged = |damage, fire, push, lift| Hit {
        damage,
        fire,
        push,
        lift,
        melee: false,
        learn: a == Attack::Breath && s.rider.is_none(),
    };
    match a {
        Attack::Bite if age == 6 => {
            if s.rider.is_some() {
                let air = mob.view().is_some_and(|v| !v.on_ground);
                let range = if air { 8.0 } else { 6.0 };
                let center = add(at, scale(f, 5.0));
                // shortcut: carrier-centered sweep; multipart body geometry needs a host hook.
                area(
                    world,
                    mob,
                    center,
                    range,
                    range,
                    melee(
                        55.0,
                        60,
                        if air { 0.4 } else { 0.25 },
                        if air { 0.2 } else { 0.08 },
                    ),
                    &mut seen,
                );
            } else {
                strike(world, mob, s.target, at, melee(55.0, 60, 0.25, 0.08));
            }
        }
        Attack::Slam if age == 15 => {
            let width = mob.body().map_or(2.5, |b| b.max[0] - b.min[0]);
            area(
                world,
                mob,
                at,
                width * 2.25,
                8.0,
                melee(40.0, 0, 1.1 + width * 0.15, 0.6),
                &mut seen,
            );
            particles(world, at, width, "minecraft:poof");
        }
        Attack::Stomp if age == 20 => {
            area(
                world,
                mob,
                at,
                18.0,
                18.0,
                melee(18.0, 0, 0.0, 0.75),
                &mut seen,
            );
        }
        Attack::Wing if age == 12 => {
            area(
                world,
                mob,
                at,
                22.0,
                22.0,
                melee(15.0, 0, 4.0, 0.5),
                &mut seen,
            );
        }
        // shortcut: sampled breath ray rather than expanding swept sections; upgrade with a shared projectile/terrain interface.
        Attack::Breath if age >= 9 => {
            if age == 9 && s.rider.is_none() {
                learning::release(mob);
            }
            let origin = add(add(at, [0.0, 3.0, 0.0]), scale(f, 3.0));
            let direction = rider_aim(world, s).or_else(|| {
                learning::predict(mob, 1.0, 2.0).map(|target| unit(sub(target, origin)))
            });
            if let Some(dir) = direction {
                let mut end = origin;
                for step in 1..=96 {
                    let p = add(origin, scale(dir, f64::from(step)));
                    if world
                        .collision_shape(crate::flight::cell(p))
                        .is_none_or(|shape| !shape.is_empty())
                    {
                        break;
                    }
                    end = p;
                    if age % 5 == 0 {
                        particles(world, p, 0.5, "minecraft:flame");
                    }
                    if age % 10 == 0 {
                        area(
                            world,
                            mob,
                            p,
                            (0.6 + 0.3 * f64::from(step)).min(8.0),
                            (0.6 + 0.3 * f64::from(step)).min(8.0),
                            ranged(40.0, 60, 0.0, 0.0),
                            &mut seen,
                        );
                    }
                }
                if age % 5 == 0 {
                    crate::vfx::breath(world, origin, end, crate::vfx::Element::Fire, 1.0);
                }
            }
        }
        Attack::Roar if matches!(age, 10 | 16 | 22) && !(s.rider.is_some() && s.skyfall_air) => {
            crate::pillars::magma_wave(world, mob, at, f, ((age - 10) / 6) as u32);
        }
        Attack::Fireball if age == s.charge + 15 => {
            let origin = add(add(at, [0.0, 3.0, 0.0]), scale(f, 4.0));
            let direction = rider_aim(world, s).or_else(|| {
                let target = learning::predict(mob, 0.0, 12.0)?;
                let ticks = length(sub(target, origin)) / 5.0;
                let predicted = learning::predict(mob, ticks, 12.0)?;
                Some(unit(sub(predicted, origin)))
            });
            if let Some(dir) = direction {
                let velocity = scale(dir, 5.0);
                if let Some(id) = MOBS.get().and_then(|mobs| {
                    mobs.spawn_controlled_projectile(
                        world,
                        "minecraft:fireball",
                        mob.id(),
                        origin,
                        velocity,
                    )
                }) {
                    if s.rider.is_none() {
                        learning::release(mob);
                        learning::defer_projectile(mob, id, 202);
                    }
                    s.shots.push(Shot {
                        id: Some(id),
                        position: origin,
                        velocity,
                        age: 0,
                        multiplier: charge_multiplier(s.charge),
                        travelled: 0.0,
                    });
                }
            }
        }
        Attack::Skyfall if (125..145).contains(&age) => {
            let radius = 48.0 * f64::from(age - 124) / 20.0;
            seen.extend(s.hit.iter().copied());
            area(
                world,
                mob,
                at,
                radius,
                radius,
                ranged(200.0, 160, 1.2, 0.4),
                &mut seen,
            );
            s.hit = seen.into_iter().collect();
            particles(world, at, radius, "minecraft:flame");
        }
        Attack::Skyfall if age == 290 && s.rider.is_none() => {
            s.phase_two = true;
            sound(world, at, "phase2_enter");
        }
        // shortcut: ground movement during leap uses host motion rather than Java's scripted arc.
        Attack::Leap if age == 20 => {
            mob.change(&EntityChange {
                mask: SET_MOTION,
                motion: [f[0] * 2.1, 1.5, f[2] * 2.1],
                ..Default::default()
            });
        }
        Attack::Leap if age == 30 => {
            area(
                world,
                mob,
                at,
                20.0,
                20.0,
                melee(50.0, 0, 5.5, 0.8),
                &mut seen,
            );
        }
        _ => {}
    }
}
fn charge_multiplier(charge: i32) -> f32 {
    if charge >= 50 {
        2.0
    } else if charge >= 25 {
        1.5
    } else {
        1.0
    }
}

fn rider_cooldown(a: Attack) -> i64 {
    match a {
        Attack::Bite | Attack::Slam | Attack::Fireball => 20,
        Attack::Wing => 3,
        Attack::Stomp => 6,
        Attack::Breath => 40,
        Attack::Roar => 50,
        Attack::Skyfall => 0,
        Attack::Leap => 140,
    }
}

fn begin_rider(world: World<'_>, mob: Entity<'_>, s: &mut State, a: Attack) -> bool {
    if s.attack.is_some() || now() < s.locked_until || s.bulldoze || now() < s.ready[a.index()] {
        return false;
    }
    let Some(v) = mob.view() else {
        return false;
    };
    s.attack = Some(a);
    s.age = 0;
    s.charge = 0;
    s.releasing = false;
    s.target = -1;
    s.right = !s.right;
    s.hit.clear();
    s.skyfall_air = !v.on_ground;
    s.skyfall_offset = 0;
    if a == Attack::Skyfall {
        s.skyfall_offset = if s.phase_two && v.on_ground { 16 } else { 0 };
        s.age = s.skyfall_offset;
        if s.skyfall_air
            && let Some(mobs) = MOBS.get()
        {
            mobs.set_no_gravity(mob, true);
        }
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion: [0.0; 3],
            ..Default::default()
        });
    }
    let cue = match a {
        Attack::Breath => "fire_breath_start",
        Attack::Fireball => "level1_charge",
        Attack::Wing => "wing_swipe",
        Attack::Stomp => "stomp",
        Attack::Skyfall if s.skyfall_air => "skyfall_air",
        Attack::Skyfall if s.skyfall_offset != 0 => "skyfall_phase2",
        _ => a.clip(),
    };
    sound(world, v.position, cue);
    true
}

fn finish_rider(world: World<'_>, mob: Entity<'_>, s: &mut State) {
    if let Some(a) = s.attack.take() {
        s.ready[a.index()] = now() + rider_cooldown(a);
        if a == Attack::Breath
            && let Some(v) = mob.view()
        {
            sound(world, v.position, "fire_breath_end");
        }
        if a == Attack::Skyfall
            && s.skyfall_air
            && let Some(mobs) = MOBS.get()
        {
            mobs.set_no_gravity(mob, false);
        }
    }
    s.releasing = false;
    s.hit.clear();
}

fn wet(world: World<'_>, mob: Entity<'_>) -> bool {
    mob.view().is_some_and(|v| {
        matches!(
            world.fluid_id(crate::flight::cell(v.position)).as_deref(),
            Some("minecraft:water" | "minecraft:flowing_water")
        )
    })
}

fn primary_is_hold(elapsed: i64) -> bool {
    // 180 ms on a 20 Hz server; companion sends held every four client ticks.
    elapsed >= 4
}

/// Ignivorus.getPrimaryRiderDualAbility and the released rider bindings.
pub fn rider_action(world: World<'_>, mob: Entity<'_>, rider: Entity<'_>, action: &str, phase: u8) {
    use moddedmc_api::player_actions::{ACTION_HELD, ACTION_PRESSED, ACTION_RELEASED};
    let Some(mobs) = MOBS.get() else {
        return;
    };
    if mobs.passengers(mob).first() != Some(&rider.id())
        || !taming::is_owner(mob, rider)
        || crate::eggs::is_juvenile(mob)
    {
        return;
    }
    let Some(v) = mob.view() else {
        return;
    };
    let mut s = State::load(mob);
    if s.rider != Some(rider.id()) {
        s.attack = None;
        s.primary_since = None;
        s.rider = Some(rider.id());
    }
    let pressed = phase == ACTION_PRESSED;
    let released = phase == ACTION_RELEASED;
    match action {
        "saintsdragons:ascend" => s.ascend = !released,
        "saintsdragons:descend" => s.descend = !released,
        "saintsdragons:accelerate" => {
            s.accelerate = !released;
            if released && s.bulldoze {
                s.bulldoze = false;
                s.bulldoze_ready = now() + 40;
                s.locked_until = now() + 20;
                sound(world, v.position, "bulldozer_exit");
            }
        }
        "saintsdragons:ability_primary" => {
            if pressed && s.attack.is_none() && now() >= s.locked_until {
                s.primary_since = Some(now());
            } else if phase == ACTION_HELD && s.primary_since.is_some() && !wet(world, mob) {
                begin_rider(world, mob, &mut s, Attack::Fireball);
                s.primary_since = None;
            } else if released {
                if let Some(start) = s.primary_since.take() {
                    let hold = primary_is_hold(now() - start);
                    if hold && !wet(world, mob) {
                        begin_rider(world, mob, &mut s, Attack::Fireball);
                    } else if !hold {
                        begin_rider(world, mob, &mut s, Attack::Roar);
                    }
                }
                if s.attack == Some(Attack::Fireball) && !s.releasing {
                    s.releasing = true;
                    s.charge = s.age.clamp(1, 95);
                    s.age = s.charge;
                    let cue = match charge_multiplier(s.charge) {
                        2.0 => "level3_shoots",
                        1.5 => "level2_shoots",
                        _ => "level1_shoots",
                    };
                    sound(world, v.position, cue);
                }
            }
        }
        "saintsdragons:ability_tertiary" => {
            if released && s.attack == Some(Attack::Breath) {
                finish_rider(world, mob, &mut s);
            } else if pressed && !s.breath_depleted && s.breath_used < 237.6 && !wet(world, mob) {
                begin_rider(world, mob, &mut s, Attack::Breath);
            }
        }
        "saintsdragons:ability_secondary" if pressed => {
            if let Some((hp, max)) = mobs.health(mob) {
                if hp < max {
                    mobs.message(
                        rider,
                        "Ignivorus must be at full health to unleash its cataclysm.",
                        true,
                    );
                } else if begin_rider(world, mob, &mut s, Attack::Skyfall) && hp > 50.0 {
                    if let Some(health) = crate::MOB_HEALTH.get() {
                        health.set(mob, 50.0);
                    }
                    mobs.message(
                        rider,
                        "Ignivorus sacrifices its vitality to fuel the inferno!",
                        true,
                    );
                }
            }
        }
        "saintsdragons:toggle_melee" if pressed && now() >= s.locked_until && !s.bulldoze => {
            if !v.on_ground {
                mobs.message(
                    rider,
                    "Ignivorus can only body slam while on the ground.",
                    true,
                );
            } else {
                s.melee_mode = !s.melee_mode;
                mobs.message(
                    rider,
                    if s.melee_mode {
                        "Secondary Melee"
                    } else {
                        "Primary Melee"
                    },
                    true,
                );
            }
        }
        "saintsdragons:flex"
            if pressed
                && v.on_ground
                && !s.phase_two
                && !wet(world, mob)
                && s.attack.is_none()
                && !s.bulldoze
                && now() >= s.locked_until
                && now() >= s.flex_ready =>
        {
            s.flex_until = now() + 170;
            s.flex_ready = now() + 200;
            s.locked_until = s.flex_until;
            s.phase_animation = None;
            mob.change(&EntityChange {
                mask: SET_MOTION,
                motion: [0.0; 3],
                ..Default::default()
            });
            sound(world, v.position, "flex");
        }
        "moddedmc:mounted_attack" if pressed => {
            let a = if !v.on_ground {
                Attack::Bite
            } else {
                match (s.phase_two, s.melee_mode) {
                    (false, false) => Attack::Bite,
                    (false, true) => Attack::Slam,
                    (true, false) => Attack::Wing,
                    (true, true) => Attack::Stomp,
                }
            };
            begin_rider(world, mob, &mut s, a);
        }
        "moddedmc:double_tap/back"
            if pressed
                && v.on_ground
                && !s.bulldoze
                && s.attack.is_none()
                && now() >= s.locked_until
                && now() >= s.phase_ready =>
        {
            s.phase_two = !s.phase_two;
            s.phase_animation = Some(s.phase_two);
            s.locked_until = now() + 13;
            if !s.phase_two {
                s.phase_ready = now() + 40;
            }
            sound(
                world,
                v.position,
                if s.phase_two {
                    "phase2_enter"
                } else {
                    "phase2_exit"
                },
            );
        }
        "moddedmc:double_tap/forward" if pressed && v.on_ground && now() >= s.locked_until => {
            if s.phase_two {
                begin_rider(world, mob, &mut s, Attack::Leap);
            } else if s.attack.is_none() && now() >= s.bulldoze_ready {
                s.bulldoze = !s.bulldoze;
                s.accelerate = s.bulldoze;
                s.locked_until = now() + if s.bulldoze { 25 } else { 20 };
                s.phase_animation = None;
                if !s.bulldoze {
                    s.bulldoze_ready = now() + 40;
                }
                sound(
                    world,
                    v.position,
                    if s.bulldoze {
                        "bulldozer_enter"
                    } else {
                        "bulldozer_exit"
                    },
                );
            }
        }
        _ => {}
    }
    s.save(mob);
}

/// Rider ability clocks run from the ambient goal, independently of the mounted movement goal.
fn tick_rider(world: World<'_>, mob: Entity<'_>, s: &mut State) -> bool {
    let Some(id) = s.rider else {
        if s.breath_used > 0.0 {
            s.breath_used = (s.breath_used - 0.6).max(0.0);
            if s.breath_used <= 192.0 {
                s.breath_depleted = false;
            }
            return true;
        }
        return false;
    };
    if !MOBS
        .get()
        .is_some_and(|m| m.passengers(mob).first() == Some(&id))
        || ENTITIES.get().and_then(|entities| {
            entities.with_entity(world, id, |rider| taming::is_owner(mob, rider))
        }) != Some(true)
    {
        finish_rider(world, mob, s);
        s.rider = None;
        s.primary_since = None;
        s.bulldoze = false;
        s.ascend = false;
        s.descend = false;
        s.accelerate = false;
        return true;
    }
    let Some(v) = mob.view() else {
        return false;
    };
    if now() < s.locked_until || s.attack == Some(Attack::Skyfall) {
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion: [0.0; 3],
            ..Default::default()
        });
    }
    if s.attack != Some(Attack::Breath) || s.age < 9 {
        s.breath_used = (s.breath_used - 0.6).max(0.0);
        if s.breath_used <= 192.0 {
            s.breath_depleted = false;
        }
    }
    if let Some(a) = s.attack {
        if matches!(a, Attack::Breath | Attack::Fireball) && wet(world, mob) {
            finish_rider(world, mob, s);
            return true;
        }
        if a == Attack::Fireball && !s.releasing {
            s.age = (s.age + 1).min(95);
            s.charge = s.age;
            if matches!(s.age, 25 | 50) {
                sound(
                    world,
                    v.position,
                    if s.age == 25 {
                        "level2_charge"
                    } else {
                        "level3_charge"
                    },
                );
            }
            return true;
        }
        s.age += 1;
        if a == Attack::Breath && s.age >= 9 {
            s.breath_used = (s.breath_used + 1.0).min(240.0);
            s.breath_depleted = s.breath_used >= 240.0;
        }
        tick_attack(world, mob, s, a, v.position, v.yaw);
        if s.age
            >= if a == Attack::Skyfall {
                280
            } else {
                a.duration(s.charge)
            }
            || (a == Attack::Breath && s.breath_depleted)
        {
            finish_rider(world, mob, s);
        }
    }
    if s.bulldoze && now() >= s.locked_until {
        let center = add(v.position, scale(forward(v.yaw), 4.5));
        let mut seen = s
            .bulldoze_hits
            .iter()
            .filter(|hit| hit.until > now())
            .map(|hit| hit.id)
            .collect::<HashSet<_>>();
        let old = seen.clone();
        area(
            world,
            mob,
            center,
            7.0,
            2.5,
            Hit {
                damage: 10.0,
                fire: 0,
                push: 1.0,
                lift: 0.2,
                melee: false,
                learn: false,
            },
            &mut seen,
        );
        s.bulldoze_hits.retain(|hit| hit.until > now());
        s.bulldoze_hits
            .extend(seen.difference(&old).map(|id| BulldozeHit {
                id: *id,
                until: now() + 5,
            }));
        // shortcut: tunnel carving needs exact protected-block/griefing checks and multipart sweep.
    }
    true
}

/// Mounted movement state used by taming's shared rider goal.
pub fn rider_movement(mob: Entity<'_>, sprint: bool) -> (bool, bool, bool, bool, f64) {
    let s = State::load(mob);
    let locked = now() < s.locked_until || matches!(s.attack, Some(Attack::Skyfall | Attack::Leap));
    let speed = match (s.phase_two, sprint || s.accelerate || s.bulldoze) {
        (false, false) => 0.25,
        (false, true) => 0.55,
        (true, false) => 0.15,
        (true, true) => 0.32,
    };
    let happiness = taming::happiness(mob).unwrap_or(100);
    let multiplier = match happiness {
        0..=30 => 0.5,
        60..=100 => 1.0,
        _ => 0.5 + 0.5 * f64::from(happiness - 30) / 30.0,
    };
    (
        locked,
        s.ascend,
        s.descend,
        s.accelerate || s.bulldoze,
        speed * multiplier,
    )
}

/// IgnivorusFireballEntity.explode: charge-scaled explosion in an AABB.
fn fireball_impact(
    world: World<'_>,
    mob: Entity<'_>,
    projectile: Entity<'_>,
    at: [f64; 3],
    multiplier: f32,
    token: u64,
) {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return;
    };
    let radius = 8.0 * f64::from(multiplier);
    let visual_scale = 4.0 * f64::from(multiplier);
    for id in entities.entities_in(world, add(at, [-radius; 3]), add(at, [radius; 3]), mob.id()) {
        entities.with_entity(world, id, |target| {
            if !abilities::enemy(world, mob, target)
                || crate::TAGS.get().is_some_and(|tags| {
                    tags.contains(
                        "entity_type",
                        "saintsdragons:immune_to/fire",
                        mobs.kind(target).unwrap_or_else(|| target.type_id()),
                    )
                })
            {
                return;
            }
            // Java explosion(direct projectile, causing dragon) selects player_explosion.
            let damaged = mobs
                .hurt_from(
                    target,
                    projectile,
                    mob,
                    "minecraft:player_explosion",
                    70.0 * multiplier,
                )
                .unwrap_or(false);
            if token != 0
                && let Some(uuid) = mobs.uuid(target)
            {
                learning::contact_token(mob, token, uuid, damaged);
            }
            mobs.set_on_fire(target, (80.0 * visual_scale) as u32);
            if let Some(view) = target.view()
                && visual_scale >= 6.0
            {
                let (strength, lift) = if visual_scale >= 8.0 {
                    (visual_scale * 0.25, 0.5)
                } else {
                    (visual_scale * 0.15, 0.3)
                };
                let push = scale(unit(sub(view.position, at)), strength);
                target.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: add(view.motion, add(push, [0.0, lift, 0.0])),
                    ..Default::default()
                });
            }
        });
    }
    particles(world, at, 2.0, "minecraft:explosion");
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            at,
            "minecraft:entity.generic.explode",
            SOUND_NEUTRAL,
            1.0 + visual_scale as f32 * 0.2,
            (0.9 / visual_scale as f32).max(0.4),
        );
    }
}

/// IgnivorusFireballEntity.motionForTick: delayed gravity by charge stage.
fn shot_motion(velocity: [f64; 3], travelled: f64, multiplier: f32) -> [f64; 3] {
    let straight = if multiplier >= 2.0 {
        85.0
    } else if multiplier >= 1.5 {
        50.0
    } else {
        25.0
    };
    if travelled >= straight {
        add(velocity, [0.0, -0.01, 0.0])
    } else {
        velocity
    }
}

/// Fireball flight and impact continue independently of the ability's recovery.
pub fn ambient(world: World<'_>, mob: Entity<'_>) {
    learning::expire(mob);
    let mut s = State::load(mob);
    let changed = tick_rider(world, mob, &mut s);
    if s.shots.is_empty() {
        if changed {
            s.save(mob);
        }
        return;
    }
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return;
    };
    s.shots.retain_mut(|shot| {
        let Some(id) = shot.id else {
            return false;
        };
        entities
            .with_entity(world, id, |projectile| {
                if projectile.projectile_owner() != Some(mob.id())
                    || projectile.type_id().trim_start_matches("minecraft:") != "fireball"
                {
                    return false;
                }
                shot.age += 1;
                shot.velocity = shot_motion(shot.velocity, shot.travelled, shot.multiplier);
                let Some(hit) =
                    mobs.step_controlled_projectile(projectile, shot.velocity, |target| {
                        abilities::enemy(world, mob, target)
                    })
                else {
                    return false;
                };
                let (p, impact, blocked) = match hit {
                    ProjectileStep::Miss(p) => (p, shot.age > 200, false),
                    ProjectileStep::Block(p) => (p, true, true),
                    ProjectileStep::Entity(p) => (p, true, false),
                };
                shot.travelled += length(sub(p, shot.position));
                shot.position = p;
                if impact {
                    let token = learning::projectile_token(mob, id);
                    fireball_impact(world, mob, projectile, p, shot.multiplier, token);
                    projectile.discard();
                    learning::settle(
                        mob,
                        token,
                        if blocked {
                            learning::Outcome::Blocked
                        } else {
                            learning::Outcome::Completed
                        },
                    );
                }
                !impact
            })
            .unwrap_or(false)
    });
    s.save(mob);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase_gates_melee_and_charge() {
        let mut s = State::default();
        assert_eq!(
            choose(&s, 1.0, 0.0, 1.0, 0, false, [1.0; 2]),
            Some(Attack::Slam)
        );
        assert_eq!(
            choose(&s, 5.0, 0.0, 1.0, 0, false, [1.0; 2]),
            Some(Attack::Bite)
        );
        s.phase_two = true;
        assert_eq!(
            choose(&s, 1.0, 0.0, 1.0, 0, false, [1.0; 2]),
            Some(Attack::Stomp)
        );
        assert_eq!(charge_multiplier(24), 1.0);
        assert_eq!(charge_multiplier(25), 1.5);
        assert_eq!(charge_multiplier(50), 2.0);
    }

    #[test]
    fn combat_state_round_trips() {
        let state = State {
            attack: Some(Attack::Skyfall),
            last: Some(Attack::Skyfall),
            age: 125,
            hit: vec![42],
            ..Default::default()
        };
        let decoded: State = fastnbt::from_bytes(&fastnbt::to_bytes(&state).unwrap()).unwrap();
        assert_eq!(decoded.attack, state.attack);
        assert_eq!(decoded.age, 125);
        assert_eq!(decoded.hit, vec![42]);
        assert_eq!(decoded.previous, None);
        let _: State = fastnbt::from_bytes(&fastnbt::to_bytes(&State::default()).unwrap()).unwrap();
    }

    #[test]
    fn rider_charge_and_cooldowns() {
        assert!(!primary_is_hold(3));
        assert!(primary_is_hold(4));
        assert_eq!(charge_multiplier(1), 1.0);
        assert_eq!(charge_multiplier(25), 1.5);
        assert_eq!(charge_multiplier(50), 2.0);
        assert_eq!(Attack::Fireball.duration(95), 110);
        assert_eq!(rider_cooldown(Attack::Fireball), 20);
        assert_eq!(rider_cooldown(Attack::Breath), 40);
        assert_eq!(rider_cooldown(Attack::Skyfall), 0);
        let s = State {
            rider: Some(23),
            primary_since: Some(42),
            attack: Some(Attack::Fireball),
            releasing: true,
            charge: 50,
            breath_used: 15.0,
            phase_two: true,
            bulldoze_hits: vec![BulldozeHit { id: 19, until: 300 }],
            ..Default::default()
        };
        let back: State = fastnbt::from_bytes(&fastnbt::to_bytes(&s).unwrap()).unwrap();
        assert_eq!(back.rider, Some(23));
        assert_eq!(back.primary_since, Some(42));
        assert!(back.releasing);
        assert_eq!(back.charge, 50);
        assert!(back.phase_two);
        assert_eq!(back.bulldoze_hits[0].until, 300);
        let aim = look_direction(90.0, -30.0);
        assert!((aim[0] + 3.0_f64.sqrt() / 2.0).abs() < 1e-6);
        assert!((aim[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn charged_fireball_delays_gravity_by_distance() {
        let velocity = [5.0, 0.0, 0.0];
        for (multiplier, distance) in [(1.0, 25.0), (1.5, 50.0), (2.0, 85.0)] {
            assert_eq!(shot_motion(velocity, distance - 0.1, multiplier), velocity);
            assert_eq!(
                shot_motion(velocity, distance, multiplier),
                [5.0, -0.01, 0.0]
            );
        }
    }
}
