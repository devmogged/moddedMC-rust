//! Ignivorus ground combat and Skyfall phase transition from 0.9.88.

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, abilities, taming};
use moddedmc_api::{
    abi::{SOUND_NEUTRAL, heightmap},
    content::World,
    entities::{Entity, EntityChange, SET_BODY_YAW, SET_MOTION, SET_YAW},
    mob_ai::goal_phase,
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
}
#[derive(Serialize, Deserialize)]
struct Shot {
    position: [f64; 3],
    velocity: [f64; 3],
    age: i32,
    multiplier: f32,
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
fn target_at(world: World<'_>, mob: Entity<'_>, id: i32) -> Option<([f64; 3], f64)> {
    ENTITIES.get()?.with_entity(world, id, |e| {
        if !abilities::enemy(mob, e) {
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
) -> Option<Attack> {
    let mut best = None;
    let mut high = f64::NEG_INFINITY;
    let mut consider = |attack: Attack, score: f64, valid: bool| {
        if !valid || score < 50.0 || s.ready[attack.index()] > time {
            return;
        }
        let score = score
            - if s.last == Some(attack) {
                28.0
            } else if s.previous == Some(attack) {
                12.0
            } else {
                0.0
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
}
fn strike(world: World<'_>, mob: Entity<'_>, id: i32, center: [f64; 3], hit: Hit) -> bool {
    let (Some(ai), Some(entities), Some(mobs)) = (MOB_AI.get(), ENTITIES.get(), MOBS.get()) else {
        return false;
    };
    entities
        .with_entity(world, id, |e| {
            if !abilities::enemy(mob, e) {
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
            ai.attack(world, mob, id, amount);
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
    s.attack.map(|a| match a {
        Attack::Breath if s.age < 9 => "fire_breath_start",
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
            s.attack = None;
            s.phase_two = false;
            s.ultimate_used = false;
            s.save(mob);
            ai.stop(mob);
        }
        goal_phase::TICK => {
            if let Some(attack) = s.attack {
                ai.stop(mob);
                s.age += 1;
                tick_attack(world, mob, &mut s, attack, view.position, view.yaw);
                if s.age >= attack.duration(s.charge) {
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
            if gap > 6.0 {
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
                choose(&s, gap, speed, 1.0, now(), crate::ai::random(100) < 65)
            } {
                s.previous = s.last;
                s.last = Some(attack);
                s.attack = Some(attack);
                s.target = id;
                s.age = 0;
                s.hit.clear();
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
    };
    let ranged = |damage, fire, push, lift| Hit {
        damage,
        fire,
        push,
        lift,
        melee: false,
    };
    match a {
        Attack::Bite if age == 6 => {
            strike(world, mob, s.target, at, melee(55.0, 60, 0.25, 0.08));
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
            if let Some((target, _)) = target_at(world, mob, s.target) {
                let origin = add(add(at, [0.0, 3.0, 0.0]), scale(f, 3.0));
                let dir = unit(sub(add(target, [0.0, 1.0, 0.0]), origin));
                for step in 1..=96 {
                    let p = add(origin, scale(dir, f64::from(step)));
                    if world
                        .collision_shape(crate::flight::cell(p))
                        .is_none_or(|shape| !shape.is_empty())
                    {
                        break;
                    }
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
            }
        }
        Attack::Roar if matches!(age, 15 | 21 | 27) => {
            let wave = (age - 15) / 6;
            for lane in -1..=1 {
                let p = add(
                    add(at, scale(f, 20.0 + 6.0 * f64::from(wave))),
                    scale([-f[2], 0.0, f[0]], 5.0 * f64::from(lane)),
                );
                let p = [
                    p[0],
                    f64::from(
                        world
                            .surface_height(
                                p[0].floor() as i32,
                                p[2].floor() as i32,
                                heightmap::MOTION_BLOCKING_NO_LEAVES,
                            )
                            .unwrap_or(p[1] as i32),
                    ),
                    p[2],
                ];
                area(
                    world,
                    mob,
                    p,
                    2.25,
                    5.5,
                    ranged(
                        18.0 + 4.0 * wave as f32,
                        80,
                        0.9 + 0.2 * f64::from(wave),
                        0.5,
                    ),
                    &mut seen,
                );
                particles(world, p, 2.0, "minecraft:lava");
                sound(world, p, "magma_pillar");
            }
        }
        Attack::Fireball if age == s.charge => {
            if let Some((target, _)) = target_at(world, mob, s.target) {
                let origin = add(add(at, [0.0, 3.0, 0.0]), scale(f, 4.0));
                s.shots.push(Shot {
                    position: origin,
                    velocity: scale(unit(sub(add(target, [0.0, 1.0, 0.0]), origin)), 5.0),
                    age: 0,
                    multiplier: charge_multiplier(s.charge),
                });
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
        Attack::Skyfall if age == 290 => {
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

/// Fireball flight and impact continue independently of the ability's recovery.
pub fn ambient(world: World<'_>, mob: Entity<'_>) {
    let mut s = State::load(mob);
    if s.shots.is_empty() {
        return;
    }
    s.shots.retain_mut(|shot| {
        shot.age += 1;
        let next = add(shot.position, shot.velocity);
        for step in 1..=20 {
            let p = add(shot.position, scale(shot.velocity, f64::from(step) / 20.0));
            let collision = world
                .collision_shape(crate::flight::cell(p))
                .is_none_or(|shape| !shape.is_empty());
            let entity = ENTITIES.get().is_some_and(|e| {
                e.entities_in(world, add(p, [-0.5; 3]), add(p, [0.5; 3]), mob.id())
                    .into_iter()
                    .any(|id| target_at(world, mob, id).is_some())
            });
            if collision || entity || shot.age >= 200 {
                area(
                    world,
                    mob,
                    p,
                    8.0 * f64::from(shot.multiplier),
                    8.0 * f64::from(shot.multiplier),
                    Hit {
                        damage: 70.0 * shot.multiplier,
                        fire: (320.0 * shot.multiplier) as u32,
                        push: 0.0,
                        lift: 0.0,
                        melee: false,
                    },
                    &mut HashSet::new(),
                );
                particles(world, p, 2.0, "minecraft:explosion");
                return false;
            }
        }
        particles(world, next, 0.5, "minecraft:flame");
        shot.position = next;
        let straight = if shot.multiplier == 2.0 {
            85.0
        } else if shot.multiplier == 1.5 {
            50.0
        } else {
            25.0
        };
        if f64::from(shot.age) * 5.0 >= straight {
            shot.velocity[1] -= 0.01;
        }
        true
    });
    s.save(mob);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase_gates_melee_and_charge() {
        let mut s = State::default();
        assert_eq!(choose(&s, 1.0, 0.0, 1.0, 0, false), Some(Attack::Slam));
        assert_eq!(choose(&s, 5.0, 0.0, 1.0, 0, false), Some(Attack::Bite));
        s.phase_two = true;
        assert_eq!(choose(&s, 1.0, 0.0, 1.0, 0, false), Some(Attack::Stomp));
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
}
