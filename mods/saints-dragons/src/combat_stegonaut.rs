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

fn enemy(mob: Entity<'_>, target: Entity<'_>) -> bool {
    abilities::enemy(mob, target)
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
            enemy(mob, target)
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
    let attack = State::load(mob).attack;
    (attack != 0).then(|| attack_spec(attack).4)
}

/// Both Java AI abilities hit their current valid target at tick 13, with no second range check.
fn hit(world: World<'_>, mob: Entity<'_>, id: i32, attack: i32) {
    let (Some(ai), Some(entities), Some(view)) = (MOB_AI.get(), ENTITIES.get(), mob.view()) else {
        return;
    };
    entities.with_entity(world, id, |target| {
        if !enemy(mob, target) {
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

type Uuid = [u8; 16];
type AuraKey = (&'static str, Uuid);
type AuraTargets = HashMap<AuraKey, HashSet<Uuid>>;
static AURA_TARGETS: std::sync::LazyLock<Mutex<AuraTargets>> =
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
                && (target_uuid == owner || taming::owner(target) == Some(owner))
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
