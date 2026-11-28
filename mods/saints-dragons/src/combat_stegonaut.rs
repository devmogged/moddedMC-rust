//! StegonautGroundCombatBehaviour, StegonautTargetingBehaviour and the tame buff aura.
use moddedmc_api::{
    RegistrationError, Registry,
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{Entity, EntityChange, SET_BODY_YAW, SET_MOTION, SET_YAW},
    mob_ai::{Goal, goal_control, goal_phase},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, abilities, eggs, flight, hunger, taming};

const KIND: &str = "saintsdragons:stegonaut";
const DATA: &str = "saintsdragons:stegonaut_combat";
pub const TARGET: u64 = 140;
pub const FIGHT: u64 = 141;
pub const AURA: u64 = 142;
const FOLLOW: f64 = 32.0;
const CADENCE: i32 = 26;

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
struct State {
    attack: i32,
    age: i32,
    cooldown: i32,
    bite_cooldown: i32,
    chin_cooldown: i32,
    last_hurt: Option<i32>,
    owner_hurt: Option<i32>,
    parent: Option<[i32; 4]>,
    poll: i32,
}

impl State {
    fn load(mob: Entity<'_>) -> Self {
        mob.data(DATA)
            .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
            .unwrap_or_default()
    }
    fn save(self, mob: Entity<'_>) {
        if let Ok(bytes) = fastnbt::to_bytes(&self) {
            mob.set_data(DATA, &bytes);
        }
    }
}

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let ai = MOB_AI.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (key, priority, controls) in [
        (TARGET, 1, goal_control::TARGET),
        (FIGHT, 1, goal_control::MOVE | goal_control::LOOK),
        (AURA, 0, 0),
    ] {
        ai.add_goal(
            registry,
            &Goal {
                kind: KIND,
                key,
                priority,
                controls,
            },
        )?;
    }
    Ok(())
}

fn stegonaut(entity: Entity<'_>) -> bool {
    MOBS.get().and_then(|mobs| mobs.kind(entity)) == Some(KIND)
}

fn enemy(world: World<'_>, mob: Entity<'_>, target: Entity<'_>) -> bool {
    abilities::enemy(world, mob, target)
        && !stegonaut(target)
        && !taming::owner(mob).is_some_and(|owner| taming::owner(target) == Some(owner))
}

fn can_fight(mob: Entity<'_>) -> bool {
    !eggs::is_juvenile(mob)
        && taming::order_name(mob) != "stay"
        && MOBS
            .get()
            .is_some_and(|mobs| mobs.passengers(mob).is_empty())
        && mob.vehicle().is_none()
}

fn uuid_words(uuid: [u8; 16]) -> [i32; 4] {
    std::array::from_fn(|i| i32::from_be_bytes(uuid[i * 4..i * 4 + 4].try_into().unwrap()))
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    flight::offset(a, b)
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
}

fn nearby(world: World<'_>, mob: Entity<'_>, radius: f64) -> Vec<i32> {
    let (Some(entities), Some(body)) = (ENTITIES.get(), mob.body()) else {
        return Vec::new();
    };
    entities.entities_in(
        world,
        body.min.map(|v| v - radius),
        body.max.map(|v| v + radius),
        mob.id(),
    )
}

fn usable(world: World<'_>, mob: Entity<'_>, id: i32, range: f64) -> bool {
    let (Some(entities), Some(view)) = (ENTITIES.get(), mob.view()) else {
        return false;
    };
    entities
        .with_entity(world, id, |target| {
            enemy(world, mob, target)
                && target
                    .view()
                    .is_some_and(|v| distance(v.position, view.position) <= range)
        })
        .unwrap_or(false)
}

/// Find the newest attacker of a juvenile assigned to this female, assigning a
/// previously unassigned juvenile to its closest female within the released box.
fn baby_threat(world: World<'_>, mob: Entity<'_>) -> (bool, Option<i32>) {
    let (Some(entities), Some(mobs), Some(ai)) = (ENTITIES.get(), MOBS.get(), MOB_AI.get()) else {
        return (false, None);
    };
    if !crate::appearance::Appearance::is_female(mob) {
        return (false, None);
    }
    let Some(mother) = mobs.uuid(mob).map(uuid_words) else {
        return (false, None);
    };
    let mut protecting = false;
    let mut newest: Option<(i32, i32)> = None;
    for id in nearby(world, mob, 16.0) {
        let result = entities
            .with_entity(world, id, |baby| {
                if !stegonaut(baby)
                    || !eggs::is_juvenile(baby)
                    || baby.flags() & moddedmc_api::entities::ENTITY_ALIVE == 0
                {
                    return None;
                }
                let mut state = State::load(baby);
                if state.parent.is_none() {
                    let (Some(view), Some(body)) = (baby.view(), baby.body()) else {
                        return None;
                    };
                    let extents = [12.0, 6.0, 12.0];
                    let lower = std::array::from_fn(|i| body.min[i] - extents[i]);
                    let upper = std::array::from_fn(|i| body.max[i] + extents[i]);
                    state.parent = entities
                        .entities_in(world, lower, upper, baby.id())
                        .into_iter()
                        .filter_map(|adult_id| {
                            entities
                                .with_entity(world, adult_id, |adult| {
                                    if !stegonaut(adult)
                                        || eggs::is_juvenile(adult)
                                        || !crate::appearance::Appearance::is_female(adult)
                                        || adult.flags() & moddedmc_api::entities::ENTITY_ALIVE == 0
                                    {
                                        return None;
                                    }
                                    Some((
                                        distance(view.position, adult.view()?.position),
                                        uuid_words(mobs.uuid(adult)?),
                                    ))
                                })
                                .flatten()
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(_, uuid)| uuid);
                    if state.parent.is_some() {
                        state.save(baby);
                    }
                }
                (state.parent == Some(mother)).then(|| ai.last_hurt_by(baby))
            })
            .flatten();
        if let Some(threat) = result {
            protecting = true;
            if let Some((attacker, timestamp)) = threat
                && newest.is_none_or(|(_, old)| timestamp > old)
                && usable(world, mob, attacker, FOLLOW)
            {
                newest = Some((attacker, timestamp));
            }
        }
    }
    (protecting, newest.map(|(id, _)| id))
}

/// Retaliation, owner defense, baby protection and compatible packmates' combat targets.
/// shortcut: owner last-attack history, active raid membership and last projectile damager
/// need host support; see parity notes.
pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(mobs), Some(entities)) = (MOB_AI.get(), MOBS.get(), ENTITIES.get()) else {
        return false;
    };
    let mut state = State::load(mob);
    let old = state;
    state.bite_cooldown = (state.bite_cooldown - 1).max(0);
    state.chin_cooldown = (state.chin_cooldown - 1).max(0);
    if !can_fight(mob) {
        ai.set_target(world, mob, None);
        if state != old {
            state.save(mob);
        }
        return false;
    }
    let (protecting, threat) = baby_threat(world, mob);
    if protecting {
        if ai.target(mob) != threat {
            ai.set_target(world, mob, threat);
        }
        if state != old {
            state.save(mob);
        }
        return false;
    }
    if let Some(id) = ai.target(mob)
        && !usable(world, mob, id, FOLLOW)
    {
        ai.set_target(world, mob, None);
    }
    let owner_hurt = taming::owner(mob)
        .and_then(|uuid| mobs.by_uuid(world, &uuid))
        .and_then(|id| {
            entities
                .with_entity(world, id, |owner| ai.last_hurt_by(owner))
                .flatten()
        });
    let self_hurt = ai.last_hurt_by(mob);
    let mut chosen = None;
    if let Some((id, stamp)) = owner_hurt
        && state.owner_hurt != Some(stamp)
        && usable(world, mob, id, FOLLOW)
    {
        state.owner_hurt = Some(stamp);
        chosen = Some(id);
    }
    if chosen.is_none() && state.poll <= 0 {
        state.poll = 20 + crate::ai::random(20) as i32;
        if !taming::is_tamed(mob) {
            for other_id in nearby(world, mob, 48.0) {
                let threat = entities
                    .with_entity(world, other_id, |other| {
                        if !stegonaut(other) || taming::is_tamed(other) || eggs::is_juvenile(other)
                        {
                            return None;
                        }
                        ai.target(other)
                            .or_else(|| ai.last_hurt_by(other).map(|(id, _)| id))
                    })
                    .flatten();
                if let Some(id) = threat
                    && usable(world, mob, id, 36.0)
                {
                    chosen = Some(id);
                    break;
                }
            }
        }
    } else {
        state.poll -= 1;
    }
    if chosen.is_none()
        && let Some((id, stamp)) = self_hurt
        && state.last_hurt != Some(stamp)
        && usable(world, mob, id, FOLLOW)
    {
        state.last_hurt = Some(stamp);
        chosen = Some(id);
    }
    if let Some(id) = chosen {
        ai.set_target(world, mob, Some(id));
    }
    if state != old {
        state.save(mob);
    }
    false
}

fn attack_range(wet: bool) -> f64 {
    if wet { 6.0 } else { 3.4 }
}

fn attack_spec(attack: i32) -> (i32, f32, f64, f64, &'static str) {
    if attack == 1 {
        (21, 5.0, 0.25, 0.06, "bite")
    } else {
        (17, 12.0, 0.3, 0.14, "chin_slam")
    }
}

fn choose_attack(bite_cooldown: i32, chin_cooldown: i32, roll: bool) -> i32 {
    match (bite_cooldown == 0, chin_cooldown == 0) {
        (true, true) => {
            if roll {
                1
            } else {
                2
            }
        }
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 0,
    }
}

fn finish_attack(state: &mut State) {
    match state.attack {
        1 => state.bite_cooldown = 16,
        2 => state.chin_cooldown = 18,
        _ => {}
    }
    state.attack = 0;
    state.age = 0;
}

/// Approximate the vanilla eye ray with collision boxes (no host sensing callback).
fn clear_sight(world: World<'_>, from: [f64; 3], to: [f64; 3]) -> bool {
    let n = (distance(from, to) * 4.0).ceil().max(1.0) as u32;
    (1..n).all(|i| {
        let point: [f64; 3] = std::array::from_fn(|axis| {
            from[axis] + (to[axis] - from[axis]) * f64::from(i) / f64::from(n)
        });
        let cell = flight::cell(point);
        let local = point.map(|v| v - v.floor());
        world.collision_shape(cell).is_some_and(|boxes| {
            !boxes
                .iter()
                .any(|b| (0..3).all(|axis| (b.min[axis]..=b.max[axis]).contains(&local[axis])))
        })
    })
}

fn play(world: World<'_>, mob: Entity<'_>, attack: i32) {
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let sound = if cfg!(feature = "original-assets") {
            if attack == 1 {
                "saintsdragons:stegonaut_bite"
            } else {
                "saintsdragons:stegonaut_chin_slam"
            }
        } else {
            "minecraft:entity.ravager.attack"
        };
        effects.play_sound(world, view.position, sound, SOUND_NEUTRAL, 1.0, 1.0);
    }
}

pub fn clip(mob: Entity<'_>) -> Option<&'static str> {
    if let Some(clip) = RiderState::load(mob).clip() {
        return Some(clip);
    }
    let attack = State::load(mob).attack;
    (attack != 0).then(|| attack_spec(attack).4)
}

/// Both Java AI abilities hit their current valid target at tick 13, with no second range check.
fn hit(world: World<'_>, mob: Entity<'_>, id: i32, attack: i32) {
    let (Some(ai), Some(entities), Some(view)) = (MOB_AI.get(), ENTITIES.get(), mob.view()) else {
        return;
    };
    entities.with_entity(world, id, |target| {
        if !enemy(world, mob, target) {
            return;
        }
        let (_, damage, push, lift, _) = attack_spec(attack);
        let damage = damage * hunger::melee_multiplier(mob);
        if attack == 2 {
            // Java adds four points to the configured eight, then uses mobAttack.
            if target
                .hurt_by(mob.id(), "minecraft:mob_attack", damage)
                .is_none()
            {
                ai.attack(world, mob, id, damage);
            }
        } else {
            ai.attack(world, mob, id, damage);
        }
        if let Some(target_view) = target.view() {
            let yaw = f64::from(view.yaw).to_radians();
            let pitch = f64::from(view.pitch).to_radians();
            target.change(&EntityChange {
                mask: SET_MOTION,
                motion: flight::add(
                    target_view.motion,
                    [
                        -yaw.sin() * pitch.cos() * push,
                        lift,
                        yaw.cos() * pitch.cos() * push,
                    ],
                ),
                ..EntityChange::default()
            });
        }
    });
}

pub fn fight(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(entities), Some(view), Some(body)) =
        (MOB_AI.get(), ENTITIES.get(), mob.view(), mob.body())
    else {
        return false;
    };
    let mut state = State::load(mob);
    let target = ai.target(mob).filter(|&id| usable(world, mob, id, FOLLOW));
    let active = can_fight(mob) && !crate::breed::wants_mate(world, mob) && target.is_some();
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => return active,
        goal_phase::STOP => {
            finish_attack(&mut state);
            state.cooldown = 0;
            state.save(mob);
            ai.stop(mob);
        }
        goal_phase::TICK => {
            let old = state;
            state.cooldown = (state.cooldown - 1).max(0);
            if state.attack != 0 {
                ai.stop(mob);
                state.age += 1;
                if state.age == 13
                    && let Some(id) = target
                {
                    hit(world, mob, id, state.attack);
                }
                if state.age >= attack_spec(state.attack).0 {
                    finish_attack(&mut state);
                }
            } else if let Some(id) = target {
                let Some((at, other)) = entities
                    .with_entity(world, id, |e| Some((e.view()?, e.body()?)))
                    .flatten()
                else {
                    return false;
                };
                let wet = world
                    .fluid_id(flight::cell(view.position))
                    .is_some_and(|s| s.ends_with("water"));
                let radii = (body.max[0] - body.min[0] + other.max[0] - other.min[0]) * 0.5;
                let gap = distance(view.position, at.position) - radii;
                let delta = flight::offset(at.position, view.position);
                let yaw = delta[2].atan2(delta[0]).to_degrees() as f32 - 90.0;
                ai.look_at(mob, [at.position[0], other.max[1], at.position[2]]);
                let turn = (yaw - view.yaw + 180.0).rem_euclid(360.0) - 180.0;
                let turned = view.yaw + turn.clamp(-12.0, 12.0);
                mob.change(&EntityChange {
                    mask: SET_YAW | SET_BODY_YAW,
                    yaw: turned,
                    body_yaw: turned,
                    ..EntityChange::default()
                });
                if gap > 3.4 {
                    if wet {
                        let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt().max(0.01);
                        mob.change(&EntityChange {
                            mask: SET_MOTION,
                            motion: delta.map(|v| v / length * 0.12),
                            ..EntityChange::default()
                        });
                    } else if !ai.navigating(mob) || moddedmc_api::tick_count().is_multiple_of(5) {
                        ai.walk_to(mob, at.position, 1.25);
                    }
                } else {
                    ai.stop(mob);
                }
                if state.cooldown == 0
                    && gap <= attack_range(wet)
                    && clear_sight(
                        world,
                        [view.position[0], body.max[1] - 0.2, view.position[2]],
                        [at.position[0], other.max[1] - 0.2, at.position[2]],
                    )
                {
                    state.attack = choose_attack(
                        state.bite_cooldown,
                        state.chin_cooldown,
                        crate::ai::random(2) == 0,
                    );
                    if state.attack != 0 {
                        state.age = 0;
                        state.cooldown = CADENCE;
                        play(world, mob, state.attack);
                        ai.stop(mob);
                    }
                }
            }
            if old != state {
                state.save(mob);
            }
        }
        _ => {}
    }
    false
}

const RIDER_DATA: &str = "saintsdragons:stegonaut_rider";
const EATING: i32 = 3;
const SHOOT: i32 = 4;
const CANCEL: i32 = 5;
const SLAM: i32 = 6;
const SLAM2: i32 = 7;
const FLEX: i32 = 8;

#[derive(Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
struct RiderState {
    action: i32,
    age: i32,
    melee: i32,
    eating_cd: i32,
    slam_cd: i32,
    flex_cd: i32,
    released: bool,
    chain: bool,
    accelerating: bool,
    jump_held: bool,
    chunks: Vec<GroundChunk>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
struct GroundChunk {
    position: [f64; 3],
    motion: [f64; 3],
    age: i32,
}

impl RiderState {
    fn load(mob: Entity<'_>) -> Self {
        mob.data(RIDER_DATA)
            .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
            .unwrap_or_default()
    }
    fn save(&self, mob: Entity<'_>) {
        if let Ok(bytes) = fastnbt::to_bytes(self) {
            mob.set_data(RIDER_DATA, &bytes);
        }
    }
    fn clip(&self) -> Option<&'static str> {
        match self.action {
            1 | 2 => Some(attack_spec(self.action).4),
            EATING if self.age >= 29 => Some("ground_eating_hold"),
            EATING => Some("ground_eating"),
            SHOOT => Some("ground_eating_shoot"),
            CANCEL => Some("ground_eating_cancel"),
            SLAM => Some("ground_slam"),
            SLAM2 => Some("ground_slam2"),
            FLEX => Some("flex"),
            _ => None,
        }
    }
    fn begin(&mut self, action: i32) {
        self.action = action;
        self.age = 0;
        self.released = false;
        self.chain = false;
    }
    fn release_eating(&mut self) {
        self.begin(if self.age >= 29 { SHOOT } else { CANCEL });
    }
    fn finish(&mut self) {
        match self.action {
            EATING | SHOOT | CANCEL => self.eating_cd = 30,
            SLAM | SLAM2 => self.slam_cd = 60,
            _ => {}
        }
        self.begin(0);
    }

    fn chain_slam(&self) -> bool {
        !self.released || self.chain
    }
}

fn look(yaw: f32, pitch: f32) -> [f64; 3] {
    let y = f64::from(yaw).to_radians();
    let p = f64::from(pitch).to_radians();
    [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
}

fn scale(a: [f64; 3], n: f64) -> [f64; 3] {
    a.map(|v| v * n)
}

fn normalized(a: [f64; 3], fallback: [f64; 3]) -> [f64; 3] {
    let len = flight::length_squared(a).sqrt();
    if len < 1e-4 {
        fallback
    } else {
        scale(a, 1.0 / len)
    }
}

fn wet(world: World<'_>, mob: Entity<'_>) -> bool {
    mob.view().is_some_and(|v| {
        world
            .fluid_id(flight::cell(v.position))
            .is_some_and(|id| id.ends_with("water"))
    })
}

fn rider_sound(world: World<'_>, mob: Entity<'_>, name: &str, volume: f32) {
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let id = if cfg!(feature = "original-assets") {
            format!("saintsdragons:stegonaut_{name}")
        } else {
            "minecraft:entity.ravager.roar".into()
        };
        effects.play_sound(world, view.position, &id, SOUND_NEUTRAL, volume, 1.0);
    }
}

/// Stegonaut's release binding uses attack, G/H hold and X mode selection.
pub fn rider_action(
    world: World<'_>,
    mob: Entity<'_>,
    player: Entity<'_>,
    action: &str,
    phase: u8,
) {
    use moddedmc_api::player_actions::{ACTION_HELD, ACTION_PRESSED, ACTION_RELEASED};
    if !taming::is_owner(mob, player)
        || eggs::is_juvenile(mob)
        || !MOBS
            .get()
            .is_some_and(|m| m.passengers(mob).first() == Some(&player.id()))
    {
        return;
    }
    let mut s = RiderState::load(mob);
    let old = s.clone();
    let key = action.strip_prefix("saintsdragons:").unwrap_or(action);
    if key == "accelerate" {
        s.accelerating = phase == ACTION_PRESSED || phase == ACTION_HELD;
    } else if key == "ability_tertiary" && phase == ACTION_RELEASED && s.action == EATING {
        s.release_eating();
        rider_sound(
            world,
            mob,
            if s.action == SHOOT {
                "ground_eating_shoot"
            } else {
                "ground_eating_cancel"
            },
            1.0,
        );
    } else if key == "ability_secondary" && s.action == SLAM {
        if phase == ACTION_RELEASED {
            s.released = true;
        }
        if phase == ACTION_PRESSED {
            s.chain = true;
        }
    } else if phase == ACTION_PRESSED && s.action == 0 && State::load(mob).attack == 0 {
        match key {
            "toggle_melee" => s.melee = 1 - s.melee,
            "moddedmc:mounted_attack" => {
                let combat = State::load(mob);
                let attack = if s.melee == 0 { 1 } else { 2 };
                if (attack == 1 && combat.bite_cooldown == 0)
                    || (attack == 2 && combat.chin_cooldown == 0)
                {
                    s.begin(attack);
                    play(world, mob, attack);
                }
            }
            "ability_tertiary" if s.eating_cd == 0 && !wet(world, mob) => {
                s.begin(EATING);
                rider_sound(world, mob, "ground_eating", 1.0);
            }
            "ability_secondary" if s.slam_cd == 0 && !wet(world, mob) => {
                s.begin(SLAM);
                rider_sound(world, mob, "slam", 1.35);
            }
            "flex"
                if s.flex_cd == 0
                    && !wet(world, mob)
                    && mob.view().is_some_and(|v| v.on_ground)
                    && taming::order_name(mob) != "stay" =>
            {
                s.begin(FLEX);
                s.flex_cd = 160;
                rider_sound(world, mob, "flex", 1.4);
            }
            _ => {}
        }
    }
    if s != old {
        s.save(mob);
    }
}

/// Ground-only movement from StegonautRiderController.
pub fn ride(mob: Entity<'_>, rider: Entity<'_>) {
    use moddedmc_api::{
        entities::SET_HEAD_YAW,
        mobs::{PLAYER_INPUT_JUMP, PLAYER_INPUT_SPRINT},
    };
    let (Some(mobs), Some(view), Some(r), Some(body)) =
        (MOBS.get(), mob.view(), rider.view(), rider.body())
    else {
        return;
    };
    let mut s = RiderState::load(mob);
    let old = s.clone();
    let keys = mobs.player_input(rider).unwrap_or(0);
    let jumping = keys & PLAYER_INPUT_JUMP != 0;
    let locked = matches!(s.action, SLAM | SLAM2 | FLEX);
    let speed = if s.accelerating || keys & PLAYER_INPUT_SPRINT != 0 {
        0.25
    } else {
        0.1
    } * hunger::speed_multiplier(taming::happiness(mob).unwrap_or(100));
    let turn = (r.yaw - view.yaw + 180.0).rem_euclid(360.0) - 180.0;
    let yaw = if locked {
        view.yaw
    } else {
        view.yaw + turn * 0.28
    };
    let f = look(yaw, 0.0);
    let right = [f[2], 0.0, -f[0]];
    let forward = body.input[2] * 0.9 * if body.input[2] < 0.0 { 0.5 } else { 1.0 };
    let mut motion = flight::add(
        scale(f, forward * speed),
        scale(right, body.input[0] * 0.5 * speed),
    );
    motion[1] = view.motion[1];
    if locked {
        motion[0] = 0.0;
        motion[2] = 0.0;
    } else if jumping && !s.jump_held && s.action == 0 && view.on_ground {
        motion = flight::add(motion, scale(f, 0.7));
        motion[1] = 0.75;
    }
    s.jump_held = jumping;
    mob.change(&EntityChange {
        mask: SET_MOTION | SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        motion,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..EntityChange::default()
    });
    mob.reset_fall_distance();
    rider.reset_fall_distance();
    if s != old {
        s.save(mob);
    }
}

/// DragonMeleeGeometry's mounted sweep uses the rider's view, dragon dimensions
/// and closest point of each target box rather than the autonomous AI target.
fn rider_melee(world: World<'_>, mob: Entity<'_>, aim: [f64; 3], attack: i32) {
    let (Some(entities), Some(v), Some(b)) = (ENTITIES.get(), mob.view(), mob.body()) else {
        return;
    };
    let (range, inflate, angle, close): (f64, f64, f64, f64) = if attack == 1 {
        (4.0, 4.0, 80.0, 1.4)
    } else {
        (5.0, 4.5, 95.0, 2.0)
    };
    let horizontal = normalized([aim[0], 0.0, aim[2]], look(v.yaw, 0.0));
    let origin = flight::add(
        [
            v.position[0],
            v.position[1] + (b.max[1] - b.min[1]) * 0.52,
            v.position[2],
        ],
        scale(horizontal, (b.max[0] - b.min[0]) * 0.35),
    );
    let end = flight::add(origin, scale(aim, range));
    let min = std::array::from_fn(|i| origin[i].min(end[i]) - inflate);
    let max = std::array::from_fn(|i| origin[i].max(end[i]) + inflate);
    for id in entities.entities_in(world, min, max, mob.id()) {
        let eligible = entities
            .with_entity(world, id, |target| {
                let Some(tb) = target.body() else {
                    return false;
                };
                let nearest = std::array::from_fn(|i| origin[i].clamp(tb.min[i], tb.max[i]));
                let delta = flight::offset(nearest, origin);
                let dist = flight::length_squared(delta).sqrt();
                let dot: f64 = normalized(delta, aim)
                    .iter()
                    .zip(aim)
                    .map(|(a, b)| a * b)
                    .sum();
                dist <= range + inflate
                    && (dist <= close
                        || (dot > 0.0 && (dot >= angle.to_radians().cos() || dist < range * 0.35)))
            })
            .unwrap_or(false);
        if eligible {
            hit(world, mob, id, attack);
        }
    }
}

fn damage_push(
    mob: Entity<'_>,
    target: Entity<'_>,
    origin: [f64; 3],
    damage: f32,
    push: f64,
    lift: f64,
    horizontal: bool,
) {
    let Some(v) = target.view() else { return };
    target.hurt_by(mob.id(), "minecraft:mob_attack", damage);
    let mut d = flight::offset(v.position, origin);
    if horizontal {
        d[1] = 0.0;
    }
    let n = normalized(d, [0.0, 0.0, 1.0]);
    target.change(&EntityChange {
        mask: SET_MOTION,
        motion: flight::add(v.motion, [n[0] * push, lift, n[2] * push]),
        ..EntityChange::default()
    });
}

fn slam(world: World<'_>, mob: Entity<'_>, second: bool) {
    let (Some(entities), Some(v), Some(b)) = (ENTITIES.get(), mob.view(), mob.body()) else {
        return;
    };
    let (radius, height, damage, push, lift) = if second {
        (20.0, 6.0, 25.0, 1.8, 0.5)
    } else {
        (10.0, 3.0, 20.0, 1.35, 0.38)
    };
    let ext = [radius, height, radius];
    let min = std::array::from_fn(|i| b.min[i] - ext[i]);
    let max = std::array::from_fn(|i| b.max[i] + ext[i]);
    for id in entities.entities_in(world, min, max, mob.id()) {
        entities.with_entity(world, id, |target| {
            if enemy(world, mob, target)
                && target
                    .view()
                    .is_some_and(|t| distance(v.position, t.position) <= radius)
            {
                damage_push(
                    mob,
                    target,
                    v.position,
                    damage * hunger::melee_multiplier(mob),
                    push,
                    lift,
                    true,
                );
            }
        });
    }
    if let Some(effects) = EFFECTS.get() {
        effects.particles(
            world,
            "minecraft:poof",
            v.position,
            [
                if second { 7.0 } else { 5.0 },
                0.2,
                if second { 7.0 } else { 5.0 },
            ],
            0.24,
            if second { 70 } else { 42 },
        );
    }
    if second {
        let f = look(v.yaw, 0.0);
        let right = [-f[2], 0.0, f[0]];
        for step in 1..=4 {
            for dir in [f, scale(f, -1.0), right, scale(right, -1.0)] {
                let mut position = flight::add(v.position, scale(dir, 4.5 * f64::from(step)));
                position[1] = (v.position[1] - 0.05).floor() + 1.0;
                crate::pillars::amethyst(
                    world,
                    mob,
                    position,
                    dir,
                    10.0 * hunger::melee_multiplier(mob),
                );
            }
        }
    }
}

/// GroundChunkEntity: gravity, drag, swept collision and a 3.2-block impact box.
/// shortcut: use particles and sampled collision in place of the native block renderer.
fn tick_chunk(world: World<'_>, mob: Entity<'_>, chunk: &mut GroundChunk) -> bool {
    let Some(entities) = ENTITIES.get() else {
        return false;
    };
    chunk.age += 1;
    chunk.motion[1] -= 0.05;
    let mut impact = false;
    let steps = (flight::length_squared(chunk.motion).sqrt() * 8.0)
        .ceil()
        .max(1.0) as usize;
    let increment = scale(chunk.motion, 1.0 / steps as f64);
    for _ in 0..steps {
        chunk.position = flight::add(chunk.position, increment);
        let p = chunk.position;
        let local = p.map(|x| x - x.floor());
        let block = world.collision_shape(flight::cell(p)).is_some_and(|boxes| {
            boxes
                .iter()
                .any(|b| (0..3).all(|i| (b.min[i]..=b.max[i]).contains(&local[i])))
        });
        let entity = entities
            .entities_in(world, p.map(|x| x - 0.3), p.map(|x| x + 0.3), mob.id())
            .into_iter()
            .any(|id| {
                entities
                    .with_entity(world, id, |t| enemy(world, mob, t))
                    .unwrap_or(false)
            });
        if block || entity {
            impact = true;
            break;
        }
    }
    chunk.motion = scale(chunk.motion, 0.99);
    if let Some(effects) = EFFECTS.get() {
        effects.particles(world, "minecraft:poof", chunk.position, [0.2; 3], 0.01, 3);
    }
    if !impact && chunk.age <= 80 {
        return true;
    }
    let p = chunk.position;
    for id in entities.entities_in(world, p.map(|x| x - 3.2), p.map(|x| x + 3.2), mob.id()) {
        entities.with_entity(world, id, |target| {
            if enemy(world, mob, target) {
                damage_push(mob, target, p, 10.0, 0.66, 0.16, false);
            }
        });
    }
    if let Some(effects) = EFFECTS.get() {
        effects.particles(world, "minecraft:poof", p, [0.54, 0.42, 0.54], 0.05, 28);
        effects.play_sound(
            world,
            p,
            "minecraft:block.stone.break",
            SOUND_NEUTRAL,
            1.2,
            0.9,
        );
    }
    false
}

fn rider_tick(world: World<'_>, mob: Entity<'_>) {
    let mut s = RiderState::load(mob);
    let old = s.clone();
    s.eating_cd = (s.eating_cd - 1).max(0);
    s.slam_cd = (s.slam_cd - 1).max(0);
    s.flex_cd = (s.flex_cd - 1).max(0);
    s.chunks.retain_mut(|chunk| tick_chunk(world, mob, chunk));
    let rider_aim = MOBS
        .get()
        .and_then(|mobs| mobs.passengers(mob).first().copied())
        .and_then(|id| {
            ENTITIES
                .get()?
                .with_entity(world, id, |r| {
                    if !taming::is_owner(mob, r) {
                        return None;
                    }
                    r.view().map(|v| look(v.yaw, v.pitch))
                })
                .flatten()
        });
    if let Some(aim) = rider_aim {
        s.age += i32::from(s.action != 0);
        match s.action {
            1 | 2 => {
                if s.age == 13 {
                    rider_melee(world, mob, aim, s.action);
                }
                if s.age >= attack_spec(s.action).0 {
                    let mut combat = State::load(mob);
                    combat.attack = s.action;
                    finish_attack(&mut combat);
                    combat.save(mob);
                    s.finish();
                }
            }
            EATING | SHOOT if wet(world, mob) => s.finish(),
            SHOOT if s.age >= 18 => {
                if let (Some(v), Some(b)) = (mob.view(), mob.body()) {
                    let origin = [
                        v.position[0],
                        v.position[1] + (b.max[1] - b.min[1]) * 0.8,
                        v.position[2],
                    ];
                    s.chunks.push(GroundChunk {
                        position: flight::add(origin, scale(aim, 1.2)),
                        motion: scale(aim, 2.5),
                        age: 0,
                    });
                }
                s.finish();
            }
            CANCEL if s.age >= 1 => s.finish(),
            SLAM => {
                if s.age == 18 {
                    slam(world, mob, false);
                }
                if s.age >= 23 {
                    if !s.chain_slam() {
                        s.finish();
                    } else {
                        s.begin(SLAM2);
                        rider_sound(world, mob, "slam2", 1.75);
                    }
                }
            }
            SLAM2 if s.age >= 25 => {
                slam(world, mob, true);
                s.finish();
            }
            FLEX if s.age >= 100 => s.finish(),
            _ => {}
        }
    } else {
        if s.action != 0 {
            s.finish();
        }
        s.accelerating = false;
        s.jump_held = false;
    }
    if s != old {
        s.save(mob);
    }
}

type Uuid = [u8; 16];
type AuraKey = (&'static str, Uuid);
type AuraTargets = HashMap<AuraKey, HashSet<Uuid>>;
static AURA_TARGETS: std::sync::LazyLock<Mutex<AuraTargets>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static PORTABLE_TARGETS: std::sync::LazyLock<Mutex<HashMap<&'static str, HashSet<Uuid>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn replace_targets(key: AuraKey, current: HashSet<Uuid>) -> Vec<Uuid> {
    let mut tracked = AURA_TARGETS.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = tracked.remove(&key).unwrap_or_default();
    let missing = previous.difference(&current).copied().collect();
    if !current.is_empty() {
        tracked.insert(key, current);
    }
    missing
}

fn clear_missing(world: World<'_>, key: AuraKey, current: HashSet<Uuid>) {
    let missing = replace_targets(key, current);
    remove_buffs(world, missing);
}

fn remove_buffs(world: World<'_>, missing: impl IntoIterator<Item = Uuid>) {
    if let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) {
        for uuid in missing {
            if let Some(id) = mobs.by_uuid(world, &uuid) {
                entities.with_entity(world, id, |target| {
                    target.remove_effect("minecraft:resistance");
                    target.remove_effect("minecraft:absorption");
                });
            }
        }
    }
}

const BINDER: &str = "saintsdragons:stegonaut_binder";

fn bound_binder(nbt: &[u8]) -> bool {
    use fastnbt::Value;
    let Ok(stack) = fastnbt::from_bytes::<HashMap<String, Value>>(nbt) else {
        return false;
    };
    // Recheck the ID in the snapshot in case the slot changed since the ID query.
    if stack.get("id") != Some(&Value::String(BINDER.into())) {
        return false;
    }
    let Some(Value::Compound(components)) = stack.get("components") else {
        return false;
    };
    let Some(Value::Compound(custom)) = components.get("minecraft:custom_data") else {
        return false;
    };
    custom.get("IsBound") == Some(&Value::Byte(1))
        && matches!(custom.get("BoundDragonUUID"), Some(Value::IntArray(uuid)) if uuid.len() == 4)
}

fn has_bound_binder(player: Entity<'_>) -> bool {
    (0..=40).any(|slot| {
        player.player_inventory_item(slot).as_deref() == Some(BINDER)
            && player
                .player_inventory_stack(slot)
                .is_some_and(|nbt| bound_binder(&nbt))
    })
}

/// StegonautBuffAbility.updateAllPortableBuffs: all players with a bound binder
/// share one target set per dimension. Refresh every 20 ticks and remove lost targets.
pub fn portable_aura(world: World<'_>, time: u64) {
    use moddedmc_api::entities::{ENTITY_ALIVE, ENTITY_LIVING, ENTITY_PLAYER};
    if !time.is_multiple_of(20) {
        return;
    }
    let (Some(dimension), Some(mobs), Some(entities)) =
        (world.dimension_id(), MOBS.get(), ENTITIES.get())
    else {
        return;
    };
    let mut current = HashSet::new();
    for id in entities.players(world) {
        entities.with_entity(world, id, |player| {
            if player.flags() & ENTITY_ALIVE == 0 || !has_bound_binder(player) {
                return;
            }
            let Some(owner) = mobs.uuid(player) else {
                return;
            };
            let mut apply = |target: Entity<'_>| {
                let Some(view) = target.view() else { return };
                if target.flags() & ENTITY_ALIVE == 0
                    || view.kind & (ENTITY_LIVING | ENTITY_PLAYER) == 0
                {
                    return;
                }
                if (view.kind & ENTITY_PLAYER != 0
                    || taming::companion_owner(target).is_some_and(|other| {
                        other == owner
                            || (stegonaut(target)
                                && crate::allies::trusts_uuid(world, other, owner))
                    }))
                    && let Some(uuid) = mobs.uuid(target)
                {
                    target.add_effect("minecraft:resistance", 40, 0, false, false);
                    target.add_effect("minecraft:absorption", 40, 0, false, false);
                    current.insert(uuid);
                }
            };
            apply(player);
            for target in nearby(world, player, 8.0) {
                entities.with_entity(world, target, &mut apply);
            }
        });
    }
    // shortcut: Ivy's own binder inventory is not exposed by the player inventory hook.
    let previous = {
        let mut tracked = PORTABLE_TARGETS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let previous = tracked.remove(dimension).unwrap_or_default();
        let missing = previous.difference(&current).copied().collect::<Vec<_>>();
        if !current.is_empty() {
            tracked.insert(dimension, current);
        }
        missing
    };
    remove_buffs(world, previous);
}

/// StegonautBuffAbility.clearAuraTargets on removal, including capture by a binder.
/// shortcut: without a removal callback, detect a missing source on the next world tick.
pub fn sweep_aura(world: World<'_>) {
    let (Some(dimension), Some(mobs), Some(entities)) =
        (world.dimension_id(), MOBS.get(), ENTITIES.get())
    else {
        return;
    };
    let keys: Vec<_> = AURA_TARGETS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .keys()
        .filter(|(dim, _)| *dim == dimension)
        .copied()
        .collect();
    for key in keys {
        let active = mobs.by_uuid(world, &key.1).is_some_and(|id| {
            entities
                .with_entity(world, id, |mob| {
                    stegonaut(mob)
                        && taming::is_tamed(mob)
                        && mob.flags() & moddedmc_api::entities::ENTITY_ALIVE != 0
                })
                .unwrap_or(false)
        });
        if !active {
            clear_missing(world, key, HashSet::new());
        }
    }
}

/// StegonautBuffAbility: every 20 ticks, renew resistance I and absorption I for 40 ticks.
/// Remove both effects from targets which leave this dragon's aura, as in Java.
pub fn aura(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    rider_tick(world, mob);
    if !moddedmc_api::tick_count().is_multiple_of(20) {
        return false;
    }
    let (Some(owner), Some(mobs), Some(entities), Some(dimension)) = (
        taming::owner(mob),
        MOBS.get(),
        ENTITIES.get(),
        world.dimension_id(),
    ) else {
        return false;
    };
    let Some(uuid) = mobs.uuid(mob) else {
        return false;
    };
    let mut current = HashSet::new();
    for id in nearby(world, mob, 8.0) {
        entities.with_entity(world, id, |target| {
            if target.flags() & moddedmc_api::entities::ENTITY_ALIVE != 0
                && let Some(target_uuid) = mobs.uuid(target)
                && (target_uuid == owner
                    || (target
                        .view()
                        .is_some_and(|v| v.kind & moddedmc_api::entities::ENTITY_PLAYER != 0)
                        && (crate::allies::trusts_uuid(world, owner, target_uuid)
                            || mobs.allied(mob, target)))
                    || taming::companion_owner(target).is_some_and(|other| {
                        other == owner
                            || (stegonaut(target)
                                && crate::allies::trusts_uuid(world, other, owner))
                            || (mobs.kind(target) == Some(crate::ivy::KIND)
                                && crate::allies::trusts_uuid(world, owner, other))
                    }))
            {
                target.add_effect("minecraft:resistance", 40, 0, false, false);
                target.add_effect("minecraft:absorption", 40, 0, false, false);
                current.insert(target_uuid);
            }
        });
    }
    clear_missing(world, (dimension, uuid), current);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_eating_requires_full_charge_and_cooldown() {
        let mut s = RiderState::default();
        s.begin(EATING);
        s.age = 28;
        s.release_eating();
        assert_eq!(s.action, CANCEL);
        s.finish();
        assert_eq!(s.eating_cd, 30);
        s.begin(EATING);
        s.age = 29;
        assert_eq!(s.clip(), Some("ground_eating_hold"));
        s.release_eating();
        assert_eq!((s.action, s.age), (SHOOT, 0));
    }

    #[test]
    fn first_slam_chains_on_hold_or_explicit_repress() {
        let mut s = RiderState::default();
        s.begin(SLAM);
        assert!(s.chain_slam());
        s.released = true;
        assert!(!s.chain_slam());
        s.chain = true;
        assert!(s.chain_slam());
        s.begin(SLAM2);
        s.finish();
        assert_eq!(s.slam_cd, 60);
        let bytes = fastnbt::to_bytes(&s).unwrap();
        assert!(fastnbt::from_bytes::<RiderState>(&bytes).unwrap() == s);
    }

    #[test]
    fn portable_aura_requires_bound_binder_with_uuid() {
        use fastnbt::{IntArray, Value};
        let encode = |id: &str, bound: i8, uuid: Vec<i32>| {
            fastnbt::to_bytes(&HashMap::from([
                ("id", Value::String(id.into())),
                (
                    "components",
                    Value::Compound(HashMap::from([(
                        "minecraft:custom_data".into(),
                        Value::Compound(HashMap::from([
                            ("IsBound".into(), Value::Byte(bound)),
                            (
                                "BoundDragonUUID".into(),
                                Value::IntArray(IntArray::new(uuid)),
                            ),
                        ])),
                    )])),
                ),
            ]))
            .unwrap()
        };
        assert!(bound_binder(&encode(BINDER, 1, vec![1, 2, 3, 4])));
        assert!(!bound_binder(&encode(BINDER, 0, vec![1, 2, 3, 4])));
        assert!(!bound_binder(&encode(BINDER, 1, vec![1, 2, 3])));
        assert!(!bound_binder(&encode(
            "minecraft:paper",
            1,
            vec![1, 2, 3, 4]
        )));
        assert!(!bound_binder(&[]));
    }

    #[test]
    fn aura_removes_only_targets_missing_from_this_source() {
        let a = [1; 16];
        let b = [2; 16];
        let key = ("test:stegonaut_aura", [3; 16]);
        assert!(replace_targets(key, HashSet::from([a, b])).is_empty());
        assert_eq!(replace_targets(key, HashSet::from([b])), vec![a]);
        assert_eq!(replace_targets(key, HashSet::new()), vec![b]);
        assert!(replace_targets(key, HashSet::new()).is_empty());
    }

    #[test]
    fn combat_nbt_preserves_cooldowns_and_parent() {
        let state = State {
            attack: 2,
            age: 13,
            cooldown: 14,
            bite_cooldown: 16,
            parent: Some(uuid_words([255; 16])),
            ..State::default()
        };
        for expected in [State::default(), state] {
            let bytes = fastnbt::to_bytes(&expected).unwrap();
            assert!(fastnbt::from_bytes::<State>(&bytes).unwrap() == expected);
        }
    }
    #[test]
    fn melee_tracks_and_water_reach() {
        assert_eq!(attack_spec(1), (21, 5.0, 0.25, 0.06, "bite"));
        assert_eq!(attack_spec(2), (17, 12.0, 0.3, 0.14, "chin_slam"));
        assert_eq!(attack_range(false), 3.4);
        assert_eq!(attack_range(true), 6.0);
        assert!(CADENCE > attack_spec(1).0);
    }

    #[test]
    fn released_ability_cooldowns_prevent_immediate_repeat() {
        let mut state = State {
            attack: 1,
            ..State::default()
        };
        finish_attack(&mut state);
        assert_eq!(state.bite_cooldown, 16);
        assert_eq!(choose_attack(state.bite_cooldown, 0, true), 2);
        state.attack = 2;
        finish_attack(&mut state);
        assert_eq!(state.chin_cooldown, 18);
        assert_eq!(
            choose_attack(state.bite_cooldown, state.chin_cooldown, true),
            0
        );
        assert_eq!(choose_attack(0, 0, false), 2);
        assert_eq!(choose_attack(0, 0, true), 1);
    }
}
