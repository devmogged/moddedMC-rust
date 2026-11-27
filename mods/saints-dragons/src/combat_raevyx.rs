//! Raevyx combat from 0.9.88: ground and air attacks, chain lightning and rider dive impact.
use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{Entity, EntityChange, EntityView, SET_BODY_YAW, SET_MOTION, SET_YAW},
    mob_ai::goal_phase,
};
use serde::{Deserialize, Serialize};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, abilities::enemy, ai::random, flight, taming};

pub const TARGETING: u64 = 120;
pub const COMBAT: u64 = 121;
const DATA: &str = "saintsdragons:raevyx_combat";
const RANGE: f64 = 64.0;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum Attack {
    Bite,
    Gore,
    Rend,
    Beam,
    Roar,
    Dash,
}

impl Attack {
    fn duration(self, burst: i32) -> i32 {
        match self {
            Self::Bite | Self::Gore => 8,
            Self::Rend => 70,
            Self::Beam => 20 + burst,
            Self::Roar => 30,
            Self::Dash => 27,
        }
    }

    fn clip(self, age: i32) -> &'static str {
        match self {
            Self::Bite => "lightning_bite",
            Self::Gore => "horn_gore",
            Self::Rend => "ground_rend",
            Self::Beam if age < 20 => "lightning_beam_start",
            Self::Beam => "lightning_beaming",
            Self::Roar => "roar",
            Self::Dash => "dash_forward_left",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct State {
    active: Option<Attack>,
    age: i32,
    target: i32,
    last_hurt: Option<i32>,
    last_owner_hurt: Option<i32>,
    cooldowns: [i32; 6],
    cadence: i32,
    major: i32,
    energy: f32,
    burst: i32,
    mobility: f32,
    decision: i32,
    roar_next: i32,
    roar_targets: Vec<i32>,
    roar_cursor: usize,
    hit_ids: Vec<i32>,
    hit_ticks: Vec<i32>,
    direction: [f64; 3],
    dive_ticks: i32,
    dive_drop: f64,
    dive_peak: f64,
    dive_rider: Option<i32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            active: None,
            age: 0,
            target: 0,
            last_hurt: None,
            last_owner_hurt: None,
            cooldowns: [0; 6],
            cadence: 0,
            major: 0,
            energy: 1.0,
            burst: 40,
            mobility: 1.0,
            decision: 0,
            roar_next: 6,
            roar_targets: Vec::new(),
            roar_cursor: 0,
            hit_ids: Vec::new(),
            hit_ticks: Vec::new(),
            direction: [0.0, 0.0, 1.0],
            dive_ticks: 0,
            dive_drop: 0.0,
            dive_peak: 0.0,
            dive_rider: None,
        }
    }
}

impl State {
    fn load(mob: Entity<'_>) -> Self {
        mob.data(DATA)
            .and_then(|nbt| fastnbt::from_bytes(&nbt).ok())
            .unwrap_or_default()
    }

    fn save(&self, mob: Entity<'_>) {
        if let Ok(nbt) = fastnbt::to_bytes(self) {
            mob.set_data(DATA, &nbt);
        }
    }

    fn ready(&self, attack: Attack) -> bool {
        self.cooldowns[attack.index()] <= 0
            && self.cadence <= 0
            && (!matches!(attack, Attack::Beam | Attack::Roar | Attack::Rend) || self.major <= 0)
    }

    fn finish(&mut self, aerial: bool) {
        if let Some(attack) = self.active.take()
            && attack == Attack::Beam
        {
            self.cooldowns[attack.index()] = if self.age <= 20 {
                40
            } else if aerial {
                160 + random(81) as i32
            } else {
                300 + random(101) as i32
            };
        }
        self.hit_ids.clear();
        self.hit_ticks.clear();
    }

    fn start(&mut self, attack: Attack, target: i32, direction: [f64; 3]) {
        self.active = Some(attack);
        self.age = 0;
        self.target = target;
        self.direction = direction;
        self.hit_ids.clear();
        self.hit_ticks.clear();
        let (cadence, cooldown, major) = match attack {
            Attack::Bite => (20, 20, 0),
            Attack::Gore => (20, 22, 0),
            Attack::Rend => (26, 400, 100),
            Attack::Beam => (12, 0, 40),
            Attack::Roar => (24, 70, 80),
            Attack::Dash => (27, 50, 0),
        };
        self.cadence = cadence;
        self.cooldowns[attack.index()] = cooldown;
        self.major = self.major.max(major);
        if attack == Attack::Beam {
            self.burst = 40 + random(41) as i32;
            self.mobility = (self.mobility - 0.25).max(0.0);
        }
        if attack == Attack::Dash {
            self.mobility = (self.mobility - 0.62).max(0.0);
        }
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    flight::length_squared(flight::offset(a, b)).sqrt()
}

fn unit(delta: [f64; 3]) -> [f64; 3] {
    let length = flight::length_squared(delta).sqrt();
    if length > 1.0e-6 {
        delta.map(|x| x / length)
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn target_view(world: World<'_>, mob: Entity<'_>, id: i32) -> Option<EntityView> {
    ENTITIES.get()?.with_entity(world, id, |target| {
        enemy(mob, target).then(|| target.view()).flatten()
    })?
}

fn center(entity: Entity<'_>) -> Option<[f64; 3]> {
    let body = entity.body()?;
    Some(std::array::from_fn(|i| (body.min[i] + body.max[i]) * 0.5))
}

fn nearby(
    world: World<'_>,
    mob: Entity<'_>,
    at: [f64; 3],
    radius: f64,
) -> Vec<(i32, [f64; 3], f64)> {
    let Some(entities) = ENTITIES.get() else {
        return Vec::new();
    };
    let mut found: Vec<_> = entities
        .entities_in(
            world,
            at.map(|x| x - radius),
            at.map(|x| x + radius),
            mob.id(),
        )
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |target| {
                    if !enemy(mob, target) {
                        return None;
                    }
                    let body = target.body()?;
                    let center = center(target)?;
                    let d = flight::length_squared(std::array::from_fn(|i| {
                        (body.min[i] - at[i]).max(at[i] - body.max[i]).max(0.0)
                    }));
                    (d <= radius * radius).then_some((id, center, d))
                })
                .flatten()
        })
        .collect();
    found.sort_by(|a, b| a.2.total_cmp(&b.2));
    found
}

fn spark(world: World<'_>, at: [f64; 3], count: u32) {
    if let Some(effects) = EFFECTS.get() {
        effects.particles(
            world,
            "minecraft:electric_spark",
            at,
            [0.15; 3],
            0.02,
            count,
        );
    }
}

/// Vanilla sparks stand in for the custom LightningVisualEntity mesh.
fn arc(world: World<'_>, from: [f64; 3], to: [f64; 3]) {
    let count = (distance(from, to) * 3.0).ceil().clamp(1.0, 192.0) as i32;
    for i in 0..=count {
        spark(
            world,
            std::array::from_fn(|j| from[j] + (to[j] - from[j]) * f64::from(i) / f64::from(count)),
            1,
        );
    }
}

fn sound(world: World<'_>, mob: Entity<'_>, name: &str, volume: f32) {
    if let Some(effects) = EFFECTS.get() {
        let original = format!("saintsdragons:raevyx_{name}");
        let event = if cfg!(feature = "original-assets") {
            original.as_str()
        } else {
            "minecraft:entity.lightning_bolt.impact"
        };
        effects.play_entity_sound(world, mob.id(), event, SOUND_NEUTRAL, volume, 1.0);
    }
}

/// The released lightning source is attributed to the dragon. The host only supports
/// unattributed typed damage, so use mobAttack for dragons and vanilla lightning otherwise.
fn lightning(world: World<'_>, mob: Entity<'_>, id: i32, amount: f32) {
    if let (Some(entities), Some(ai), Some(mobs)) = (ENTITIES.get(), MOB_AI.get(), MOBS.get()) {
        entities.with_entity(world, id, |target| {
            if !enemy(mob, target) || mobs.kind(target) == Some("saintsdragons:raevyx") {
                return;
            }
            if mobs
                .kind(target)
                .is_some_and(|kind| kind.starts_with("saintsdragons:"))
            {
                ai.attack(world, mob, id, amount);
            } else {
                target.hurt("minecraft:lightning_bolt", amount);
            }
        });
    }
}

/// RaevyxChainLightningAbility: nearest box, no revisits, falloff, isolated discharge.
fn chain(
    world: World<'_>,
    mob: Entity<'_>,
    start: i32,
    base: f32,
    radius: f64,
    jumps: i32,
    falloff: f32,
) {
    let Some(entities) = ENTITIES.get() else {
        return;
    };
    let Some(mut at) = entities.with_entity(world, start, center).flatten() else {
        return;
    };
    let mut hit = vec![start];
    let mut amount = base;
    for index in 0..jumps {
        let next = nearby(world, mob, at, radius)
            .into_iter()
            .find(|(id, _, _)| {
                !hit.contains(id)
                    && entities
                        .with_entity(world, *id, |target| {
                            MOBS.get().and_then(|mobs| mobs.kind(target))
                                != Some("saintsdragons:raevyx")
                        })
                        .unwrap_or(false)
            });
        let Some((id, next_at, _)) = next else {
            if index == 0 {
                lightning(world, mob, start, amount);
                spark(world, at, 10);
            }
            break;
        };
        lightning(world, mob, id, amount);
        arc(world, at, next_at);
        hit.push(id);
        at = next_at;
        amount *= falloff;
    }
}

fn melee(world: World<'_>, mob: Entity<'_>, id: i32, amount: f32, push: [f64; 3]) {
    if let (Some(ai), Some(entities)) = (MOB_AI.get(), ENTITIES.get()) {
        entities.with_entity(world, id, |target| {
            if enemy(mob, target)
                && ai.attack(
                    world,
                    mob,
                    id,
                    amount * crate::hunger::melee_multiplier(mob),
                )
                && let Some(view) = target.view()
            {
                target.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: flight::add(view.motion, push),
                    ..Default::default()
                });
            }
        });
    }
}

fn clear_line(world: World<'_>, from: [f64; 3], to: [f64; 3]) -> bool {
    let steps = (distance(from, to) * 4.0).ceil().max(1.0) as i32;
    (1..steps).all(|i| {
        let p =
            std::array::from_fn(|j| from[j] + (to[j] - from[j]) * f64::from(i) / f64::from(steps));
        world
            .collision_shape(flight::cell(p))
            .is_some_and(|shape| shape.is_empty())
    })
}

fn can_fight(mob: Entity<'_>) -> bool {
    !crate::eggs::is_juvenile(mob)
        && taming::order_name(mob) != "stay"
        && MOBS
            .get()
            .is_some_and(|mobs| mobs.passengers(mob).is_empty())
        && mob.vehicle().is_none()
}

pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return false;
    };
    if !can_fight(mob) {
        ai.set_target(world, mob, None);
        return false;
    }
    let mut state = State::load(mob);
    let valid = |id| {
        target_view(world, mob, id).is_some_and(|v| distance(view.position, v.position) <= RANGE)
    };
    if ai.target(mob).is_some_and(|id| !valid(id)) {
        ai.set_target(world, mob, None);
    }
    let owner_hurt = taming::owner(mob)
        .and_then(|uuid| mobs.by_uuid(world, &uuid))
        .and_then(|id| {
            entities
                .with_entity(world, id, |owner| ai.last_hurt_by(owner))
                .flatten()
        });
    if let Some((id, stamp)) = owner_hurt
        && state.last_owner_hurt != Some(stamp)
        && valid(id)
    {
        state.last_owner_hurt = Some(stamp);
        ai.set_target(world, mob, Some(id));
    } else if let Some((id, stamp)) = ai.last_hurt_by(mob)
        && state.last_hurt != Some(stamp)
        && valid(id)
    {
        state.last_hurt = Some(stamp);
        ai.set_target(world, mob, Some(id));
    }
    state.save(mob);
    false
}

/// GroundCombatBehaviour melee order and AirCombatBehaviour's bite/ranged thresholds.
fn choose(state: &State, gap: f64, aerial: bool, beam_ok: bool, roll: u64) -> Option<Attack> {
    if !aerial && (3.4..=8.5).contains(&gap) && state.ready(Attack::Rend) {
        return Some(Attack::Rend);
    }
    if gap <= if aerial { 7.0 } else { 3.0 } && state.ready(Attack::Bite) {
        return Some(Attack::Bite);
    }
    if !aerial && gap <= 4.5 && state.ready(Attack::Gore) {
        return Some(Attack::Gore);
    }
    if beam_ok
        && gap >= if aerial { 16.0 } else { 12.0 }
        && gap <= 54.4
        && state.energy >= 0.6
        && state.mobility >= 0.25
        && state.ready(Attack::Beam)
        && roll < 65
    {
        return Some(Attack::Beam);
    }
    if gap <= if aerial { 40.0 } else { 18.0 }
        && (!aerial || gap >= 6.0)
        && state.ready(Attack::Roar)
        && roll < 5
    {
        return Some(Attack::Roar);
    }
    if !aerial && (8.0..=26.0).contains(&gap) && state.mobility >= 0.62 && state.ready(Attack::Dash)
    {
        return Some(Attack::Dash);
    }
    None
}

fn gap(mob: Entity<'_>, world: World<'_>, id: i32, view: EntityView, target: EntityView) -> f64 {
    let width = ENTITIES
        .get()
        .and_then(|e| e.with_entity(world, id, |target| target.body()).flatten())
        .map_or(0.6, |b| b.max[0] - b.min[0]);
    let own = mob.body().map_or(2.5, |b| b.max[0] - b.min[0]);
    (distance(view.position, target.position) - (width + own) * 0.5).max(0.0)
}

fn turn(mob: Entity<'_>, direction: [f64; 3], motion: Option<[f64; 3]>) {
    let yaw = direction[2].atan2(direction[0]).to_degrees() as f32 - 90.0;
    mob.change(&EntityChange {
        mask: SET_YAW | SET_BODY_YAW | if motion.is_some() { SET_MOTION } else { 0 },
        yaw,
        body_yaw: yaw,
        motion: motion.unwrap_or_default(),
        ..Default::default()
    });
}

pub fn combat(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(view)) = (MOB_AI.get(), mob.view()) else {
        return false;
    };
    let target = ai
        .target(mob)
        .and_then(|id| target_view(world, mob, id).map(|v| (id, v)));
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => can_fight(mob) && target.is_some(),
        goal_phase::STOP => {
            let mut state = State::load(mob);
            state.finish(!view.on_ground);
            state.save(mob);
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            let Some((id, target)) = target else {
                return false;
            };
            let mut state = State::load(mob);
            let gap = gap(mob, world, id, view, target);
            let aerial = !view.on_ground;
            let delta = flight::offset(target.position, view.position);
            let dir = unit([delta[0], 0.0, delta[2]]);
            if let Some(attack) = state.active {
                ai.stop(mob);
                tick_attack(world, mob, &mut state, attack, view, target, gap);
                state.save(mob);
                return false;
            }
            let from = center(mob).unwrap_or(view.position);
            let to = ENTITIES
                .get()
                .and_then(|e| e.with_entity(world, id, center).flatten())
                .unwrap_or(target.position);
            let visible = clear_line(world, from, to);
            let beam_ok = visible
                && MOBS
                    .get()
                    .and_then(|m| {
                        ENTITIES
                            .get()?
                            .with_entity(world, id, |t| m.health(t))
                            .flatten()
                    })
                    .is_some_and(|(hp, max)| hp > max * 0.25);
            if state.decision <= 0 {
                state.decision = 10;
                if visible && let Some(attack) = choose(&state, gap, aerial, beam_ok, random(100)) {
                    state.start(attack, id, dir);
                    if attack == Attack::Roar {
                        state.roar_targets = nearby(world, mob, view.position, 24.0)
                            .into_iter()
                            .filter(|(candidate, _, _)| {
                                *candidate == id
                                    || ENTITIES.get().is_some_and(|e| {
                                        e.with_entity(world, *candidate, |t| {
                                            ai.target(t) == Some(mob.id())
                                        })
                                        .unwrap_or(false)
                                    })
                            })
                            .map(|(id, _, _)| id)
                            .take(6)
                            .collect();
                        if !state.roar_targets.contains(&id) {
                            state.roar_targets.push(id);
                        }
                        state.roar_cursor = 0;
                        state.roar_next = 6;
                    }
                    sound(
                        world,
                        mob,
                        match attack {
                            Attack::Bite => "bite",
                            Attack::Gore => "horngore",
                            Attack::Rend => "ground_rend",
                            Attack::Beam => "lightning_beam_start",
                            Attack::Roar => "roar",
                            Attack::Dash => "dash",
                        },
                        1.3,
                    );
                    ai.stop(mob);
                    turn(mob, dir, None);
                    state.save(mob);
                    return false;
                }
            }
            ai.look_at(mob, to);
            if aerial {
                // shortcut: direct chase, rather than the Java orbit/commit/breakaway flight planner.
                let motion = unit(delta).map(|x| x * 0.55);
                turn(mob, dir, Some(motion));
                mob.reset_fall_distance();
            } else if gap > 4.5 || !visible {
                if !ai.navigating(mob) || moddedmc_api::tick_count().is_multiple_of(5) {
                    ai.walk_to(mob, target.position, 1.45);
                }
            } else {
                ai.stop(mob);
                turn(mob, dir, None);
            }
            state.save(mob);
            false
        }
        _ => false,
    }
}

fn tick_attack(
    world: World<'_>,
    mob: Entity<'_>,
    state: &mut State,
    attack: Attack,
    view: EntityView,
    target: EntityView,
    gap: f64,
) {
    state.age += 1;
    let age = state.age;
    let id = state.target;
    let dir = state.direction;
    if attack == Attack::Rend {
        crate::particles::rend(world, mob, view, age, dir);
    }
    match attack {
        Attack::Bite if age == 3 => {
            if target_view(world, mob, id).is_some() {
                melee(world, mob, id, 15.0, [0.0; 3]);
                chain(world, mob, id, 10.0, 8.0, 5, 0.75);
            }
        }
        Attack::Gore if age == 3 => {
            // shortcut: raw damage until target armor and toughness can be read.
            melee(world, mob, id, 15.0, [dir[0] * 1.4, 0.35, dir[2] * 1.4]);
        }
        Attack::Rend if (20..55).contains(&age) => {
            if !view.on_ground {
                state.finish(true);
                return;
            }
            if gap > 6.0 && age < 50 {
                let d = flight::offset(target.position, view.position);
                state.direction = unit([d[0], 0.0, d[2]]);
            }
            let dir = state.direction;
            let speed = 0.9 + (0.1125 - 0.9) * ((age - 25) as f64 / 30.0).clamp(0.0, 1.0);
            turn(
                mob,
                dir,
                Some([dir[0] * speed, view.motion[1], dir[2] * speed]),
            );
            if age < 50 {
                let at = flight::add(view.position, [dir[0] * 2.0, 1.0, dir[2] * 2.0]);
                for (hit, _, _) in nearby(world, mob, at, 3.0) {
                    if let Some(index) = state.hit_ids.iter().position(|&old| old == hit) {
                        if age - state.hit_ticks[index] < 5 {
                            continue;
                        }
                        state.hit_ticks[index] = age;
                    } else {
                        state.hit_ids.push(hit);
                        state.hit_ticks.push(age);
                    }
                    melee(world, mob, hit, 5.0, [dir[0] * 0.55, 0.0, dir[2] * 0.55]);
                }
                if age <= 40 {
                    spark(world, at, 5);
                }
            }
        }
        Attack::Beam => {
            let Some(entities) = ENTITIES.get() else {
                return;
            };
            let Some((to, hp, max)) = entities
                .with_entity(world, id, |t| {
                    let (hp, max) = MOBS.get()?.health(t)?;
                    Some((center(t)?, hp, max))
                })
                .flatten()
            else {
                state.finish(!view.on_ground);
                return;
            };
            let from = center(mob).unwrap_or(view.position);
            if hp <= max * 0.25
                || gap <= 4.5
                || distance(from, to) > 64.0
                || !clear_line(world, from, to)
            {
                state.finish(!view.on_ground);
                sound(world, mob, "lightning_beam_stop", 1.0);
                return;
            }
            if age >= 20 {
                state.energy = (state.energy - 0.014).max(0.0);
                if state.energy <= 0.0 {
                    state.finish(!view.on_ground);
                    return;
                }
                if age == 20 {
                    sound(world, mob, "lightning_beaming", 1.3);
                }
                lightning(world, mob, id, 35.0_f32.min((hp - max * 0.25).max(0.0)));
                if age % 3 == 0 {
                    arc(world, from, to);
                }
            }
        }
        Attack::Roar if (6..18).contains(&age) && age >= state.roar_next => {
            if let Some(entities) = ENTITIES.get() {
                let count = state.roar_targets.len();
                for _ in 0..count {
                    let id = state.roar_targets[state.roar_cursor % count];
                    state.roar_cursor += 1;
                    if target_view(world, mob, id).is_none() {
                        continue;
                    }
                    lightning(world, mob, id, 5.0);
                    entities.with_entity(world, id, |t| {
                        t.add_effect("minecraft:slowness", 30, 5, false, true);
                        t.add_effect("minecraft:weakness", 30, 0, false, true);
                        t.add_effect("minecraft:blindness", 20, 0, false, true);
                        if let Some(at) = center(t) {
                            spark(world, at, 12);
                        }
                    });
                    break;
                }
            }
            state.roar_next = age + 7 + random(6) as i32;
        }
        Attack::Dash => {
            let speed = 30.0 / 27.0;
            turn(
                mob,
                dir,
                Some([dir[0] * speed, view.motion[1], dir[2] * speed]),
            );
            for (hit, _, _) in nearby(world, mob, view.position, 2.5) {
                if !state.hit_ids.contains(&hit) {
                    state.hit_ids.push(hit);
                    melee(world, mob, hit, 10.0, [dir[0], 0.2, dir[2]]);
                }
            }
        }
        _ => {}
    }
    if age >= attack.duration(state.burst) {
        if attack == Attack::Beam {
            sound(world, mob, "lightning_beam_stop", 1.0);
        }
        state.finish(!view.on_ground);
    }
}

/// RaevyxDiveImpactAbility: scale from peak downward flight speed, and chain from nearest hit.
fn impact_stats(speed: f64) -> (f64, f32, f64, f64) {
    let power = ((speed - 1.45) / (3.25 - 1.45)).clamp(0.0, 1.0);
    (
        power,
        (8.0 + (speed - 1.45) * 20.0).clamp(8.0, 40.0) as f32,
        7.0 + 5.0 * power,
        0.8 + 1.2 * power,
    )
}

fn impact(world: World<'_>, mob: Entity<'_>, at: [f64; 3], speed: f64) {
    let (power, damage, radius, knockback) = impact_stats(speed);
    crate::particles::dive_impact(world, at, power);
    let targets = nearby(world, mob, at, radius);
    for &(id, pos, _) in &targets {
        let d = flight::offset(pos, at);
        let outward = unit([d[0], 0.0, d[2]]);
        melee(
            world,
            mob,
            id,
            damage,
            [
                outward[0] * knockback,
                0.3 + power * 0.35,
                outward[2] * knockback,
            ],
        );
    }
    if let Some(&(id, _, _)) = targets.first() {
        chain(world, mob, id, (7.0 + 8.0 * power) as f32, 12.0, 4, 0.8);
    }
    spark(world, at, 35);
    sound(world, mob, "dive_impact", 2.0);
}

/// Tick energy and cooldowns even outside the combat goal. Rider diving is inferred from
/// the stock client's downward movement; custom dive input is not exposed by the host.
pub fn ambient(world: World<'_>, mob: Entity<'_>, view: EntityView) -> Option<&'static str> {
    let mut state = State::load(mob);
    for cooldown in &mut state.cooldowns {
        *cooldown = (*cooldown - 1).max(0);
    }
    state.cadence = (state.cadence - 1).max(0);
    state.major = (state.major - 1).max(0);
    state.decision = (state.decision - 1).max(0);
    state.mobility = (state.mobility + 0.025).min(1.0);
    if state.active != Some(Attack::Beam) {
        state.energy = (state.energy + 0.0025).min(1.0);
    }
    let rider = MOBS
        .get()
        .and_then(|mobs| mobs.passengers(mob).first().copied())
        .filter(|id| {
            ENTITIES.get().is_some_and(|e| {
                e.with_entity(world, *id, |r| taming::is_owner(mob, r))
                    .unwrap_or(false)
            })
        });
    if rider.is_some() && state.dive_rider == rider && view.on_ground {
        if state.dive_ticks >= 8 && state.dive_drop >= 12.0 && state.dive_peak >= 1.45 {
            impact(world, mob, view.position, state.dive_peak);
        }
        state.dive_ticks = 0;
        state.dive_drop = 0.0;
        state.dive_peak = 0.0;
    } else if rider.is_some() && !view.on_ground && view.motion[1] < -0.25 {
        state.dive_ticks += 1;
        state.dive_drop -= view.motion[1];
        state.dive_peak = state
            .dive_peak
            .max(flight::length_squared(view.motion).sqrt());
    } else {
        if rider.is_some()
            && state.dive_rider == rider
            && state.dive_ticks >= 8
            && !view.on_ground
            && let Some(owner) = taming::owner(mob)
        {
            crate::advancements::award_uuid(world, owner, "raevyx_dive_exit", "raevyx_dive_exit");
        }
        state.dive_ticks = 0;
        state.dive_drop = 0.0;
        state.dive_peak = 0.0;
    }
    state.dive_rider = rider;
    let clip = state.active.map(|attack| attack.clip(state.age));
    state.save(mob);
    clip
}

pub fn immune(mob: Entity<'_>, damage_type: &str) -> bool {
    matches!(
        damage_type,
        "minecraft:fall" | "minecraft:lightning_bolt" | "saintsdragons:raevyx_lightning"
    ) || State::load(mob).active == Some(Attack::Rend)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combat_state_roundtrips() {
        let mut state = State::default();
        let initial = fastnbt::to_bytes(&state).unwrap();
        let _: State = fastnbt::from_bytes(&initial).unwrap();
        state.start(Attack::Bite, 42, [1.0, 0.0, 0.0]);
        state.hit_ids.push(42);
        state.hit_ticks.push(3);
        let encoded = fastnbt::to_bytes(&state).unwrap();
        let restored: State = fastnbt::from_bytes(&encoded).unwrap();
        assert_eq!(restored.active, Some(Attack::Bite));
        assert_eq!(restored.target, 42);
        assert_eq!(restored.hit_ids, vec![42]);
    }

    #[test]
    fn ground_and_air_selection_respects_ranges_and_cooldowns() {
        let mut s = State::default();
        assert_eq!(choose(&s, 2.0, false, true, 99), Some(Attack::Bite));
        assert_eq!(choose(&s, 5.0, false, true, 99), Some(Attack::Rend));
        s.cooldowns[Attack::Rend.index()] = 400;
        assert_eq!(choose(&s, 4.0, false, true, 99), Some(Attack::Gore));
        assert_eq!(choose(&s, 6.0, true, true, 99), Some(Attack::Bite));
        assert_eq!(choose(&s, 20.0, true, true, 0), Some(Attack::Beam));
        assert_eq!(choose(&s, 20.0, true, false, 99), None);
        s.energy = 0.5;
        assert_ne!(choose(&s, 20.0, true, true, 0), Some(Attack::Beam));
    }

    #[test]
    fn dive_stats_match_released_minimum_and_cap() {
        assert_eq!(impact_stats(1.45), (0.0, 8.0, 7.0, 0.8));
        assert_eq!(impact_stats(3.25), (1.0, 40.0, 12.0, 2.0));
    }
}
