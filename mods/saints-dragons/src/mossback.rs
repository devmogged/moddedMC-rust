//! Mossback.java and MossbackItem.java (0.9.88).
use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{ENTITY_LIVING, Entity, EntityChange, EntityView, SET_MOTION, SET_YAW},
    interaction::{UseItem, UseMob, UseResult},
    mob_ai::goal_phase,
};
use serde::{Deserialize, Serialize};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOB_HEALTH, MOBS, ai, eggs, flight};

pub const KIND: &str = "saintsdragons:mossback";
pub const FLOAT: u64 = 190;
pub const PANIC: u64 = 191;
pub const BREED: u64 = 192;
pub const WANDER: u64 = 193;
const KEY: &str = "saintsdragons:mossback";
const GROW_TICKS: i32 = 24_000;
const COOLDOWN: i32 = 6_000;
const LOVE_TICKS: i32 = 600;
const TOXIN_TICKS: i32 = 200;
const TOXIN_RADIUS: f64 = 3.25;

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
struct State {
    age: i32,
    love: i32,
    courtship: i32,
    thrown: bool,
    landed: i32,
    panic: i32,
}

impl State {
    fn baby(self) -> bool {
        self.age < 0
    }

    fn ready(self) -> bool {
        self.age == 0 && self.love > 0 && !self.thrown
    }

    fn tick(&mut self) -> bool {
        let baby = self.baby();
        self.age -= self.age.signum();
        self.love = (self.love - 1).max(0);
        self.landed = (self.landed - 1).max(0);
        self.panic = (self.panic - 1).max(0);
        baby && !self.baby()
    }
}

fn state(mob: Entity<'_>) -> State {
    mob.data(KEY)
        .and_then(|data| fastnbt::from_bytes(&data).ok())
        .unwrap_or_default()
}

fn save(mob: Entity<'_>, state: State) -> bool {
    fastnbt::to_bytes(&state).is_ok_and(|bytes| mob.set_data(KEY, &bytes))
}

pub fn baby(mob: Entity<'_>) -> bool {
    state(mob).baby()
}

fn set_baby(mob: Entity<'_>) -> bool {
    let Some(health) = MOB_HEALTH.get() else {
        return false;
    };
    health.set_attribute(mob, "minecraft:max_health", 1.0)
        && health.set_attribute(mob, "minecraft:scale", 0.5)
        && health.set(mob, 1.0)
        && save(
            mob,
            State {
                age: -GROW_TICKS,
                ..State::default()
            },
        )
}

fn random_unit() -> f64 {
    ai::random(1 << 24) as f64 / (1 << 24) as f64
}

fn sound(world: World<'_>, at: [f64; 3], original: &str, fallback: &str, volume: f32, pitch: f32) {
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            at,
            if cfg!(feature = "original-assets") {
                original
            } else {
                fallback
            },
            SOUND_NEUTRAL,
            volume,
            pitch,
        );
    }
}

/// Mossback.releaseToxin: inflate the collision box, affect every other living entity.
fn toxin(world: World<'_>, mob: Entity<'_>) {
    let (Some(entities), Some(body), Some(view)) = (ENTITIES.get(), mob.body(), mob.view()) else {
        return;
    };
    let radius = [TOXIN_RADIUS, 1.4, TOXIN_RADIUS];
    let min = std::array::from_fn(|i| body.min[i] - radius[i]);
    let max = std::array::from_fn(|i| body.max[i] + radius[i]);
    for id in entities.entities_in(world, min, max, mob.id()) {
        entities.with_entity(world, id, |target| {
            if target.view().is_some_and(|v| v.kind & ENTITY_LIVING != 0) {
                // Mossback.canBeAffected rejects poison, including another Mossback's toxin.
                if MOBS.get().and_then(|m| m.kind(target)) != Some(KIND) {
                    target.add_effect("minecraft:poison", TOXIN_TICKS, 0, false, true);
                }
                target.add_effect("minecraft:nausea", TOXIN_TICKS, 0, false, true);
                target.add_effect("minecraft:blindness", TOXIN_TICKS, 0, false, true);
            }
        });
    }
    if let Some(effects) = EFFECTS.get() {
        // shortcut: custom particle registration is unavailable; use vanilla sneeze fumes.
        for _ in 0..36 {
            let angle = random_unit() * std::f64::consts::TAU;
            let speed = 0.035 + random_unit() * 0.085;
            let at = [
                view.position[0] + (random_unit() - 0.5) * 0.9,
                view.position[1] + 0.25 + random_unit() * 0.35,
                view.position[2] + (random_unit() - 0.5) * 0.9,
            ];
            effects.particles(
                world,
                "minecraft:sneeze",
                at,
                [
                    angle.cos() * speed,
                    0.025 + random_unit() * 0.055,
                    angle.sin() * speed,
                ],
                1.0,
                0,
            );
        }
    }
    sound(
        world,
        view.position,
        "saintsdragons:mossback_toxin",
        "minecraft:entity.puffer_fish.blow_up",
        0.9,
        0.9 + random_unit() as f32 * 0.18,
    );
}

pub fn hurt(world: World<'_>, mob: Entity<'_>, damage_type: &str, amount: f32) -> bool {
    if damage_type == "minecraft:fall" {
        return false;
    }
    if damage_type == "minecraft:generic_kill" && baby(mob) && state(mob).landed > 0 {
        return true;
    }
    if amount > 0.0 {
        // shortcut: host hook is before hurt acceptance; need post-hurt callback for exact timing.
        toxin(world, mob);
        let mut s = state(mob);
        s.panic = 100;
        save(mob, s);
        crate::audio::hurt(world, mob, KIND);
    }
    true
}

/// Mossback.tick, including landing toxin and age attributes.
pub fn ambient(world: World<'_>, mob: Entity<'_>, view: EntityView) {
    let mut s = state(mob);
    let before = s;
    if s.tick()
        && let Some(health) = MOB_HEALTH.get()
    {
        health.set_attribute(mob, "minecraft:max_health", 10.0);
        health.set_attribute(mob, "minecraft:scale", 1.0);
        health.set(mob, 10.0);
    }
    if s.thrown && view.on_ground {
        s.thrown = false;
        s.landed = 12;
        save(mob, s);
        if let Some(effects) = EFFECTS.get() {
            let mut at = view.position;
            at[1] += 0.15;
            effects.particles(world, "minecraft:poof", at, [0.18, 0.08, 0.18], 0.02, 8);
        }
        sound(
            world,
            view.position,
            "saintsdragons:mossback_landed",
            "minecraft:entity.generic.small_fall",
            0.7,
            0.95 + random_unit() as f32 * 0.12,
        );
        toxin(world, mob);
        if s.baby() {
            // The health setter refuses zero; use the host's lethal damage path.
            mob.hurt("minecraft:generic_kill", 1000.0);
            return;
        }
    }
    if s != before {
        save(mob, s);
    }
    mob.reset_fall_distance();
    if view.on_ground && view.motion[0].powi(2) + view.motion[2].powi(2) > 0.0001 {
        mob.change(&EntityChange {
            mask: SET_YAW,
            yaw: view.body_yaw,
            ..Default::default()
        });
    }
    let clip = if s.thrown {
        "thrown"
    } else if s.landed > 0 {
        "landed"
    } else if view.motion[0].hypot(view.motion[2]) > 0.035 {
        "walk"
    } else {
        "idle"
    };
    crate::visuals::tick_species(
        world,
        mob,
        if s.baby() {
            "baby_mossback"
        } else {
            "mossback"
        },
        view,
        clip,
    );
}

fn captured_stack(baby: bool) -> Vec<u8> {
    let mut stack = HashMap::from([("id", Value::String(KIND.into())), ("count", Value::Int(1))]);
    if baby {
        stack.insert(
            "components",
            Value::Compound(HashMap::from([(
                "minecraft:custom_data".into(),
                Value::Compound(HashMap::from([("BabyMossback".into(), Value::Byte(1))])),
            )])),
        );
    }
    fastnbt::to_bytes(&stack).unwrap_or_default()
}

fn captured_baby(stack: &[u8]) -> bool {
    let Ok(Value::Compound(root)) = fastnbt::from_bytes::<Value>(stack) else {
        return false;
    };
    let Some(Value::Compound(components)) = root.get("components") else {
        return false;
    };
    let Some(Value::Compound(data)) = components.get("minecraft:custom_data") else {
        return false;
    };
    matches!(data.get("BabyMossback"), Some(Value::Byte(1)))
}

/// Mossback.mobInteract and Animal.mobInteract (pufferfish).
pub fn interact(click: &mut UseMob<'_>) -> UseResult {
    let Some(mob) = click.mob else {
        return UseResult::Pass;
    };
    let mut s = state(mob);
    if click.stack.count() == 0 {
        if click.give(&captured_stack(s.baby())) {
            crate::visuals::death(click.world, mob);
            mob.discard();
            return UseResult::Success;
        }
        return UseResult::Fail;
    }
    if crate::taming::held_item(click).as_deref() != Some("minecraft:pufferfish") {
        // The wolf carrier would otherwise accept bones and become tamed.
        return UseResult::Fail;
    }
    if s.baby() {
        // AgeableMob.ageUp: ten percent of remaining seconds.
        s.age += (-s.age / 20 / 10) * 20;
    } else if s.age == 0 && s.love == 0 {
        s.love = LOVE_TICKS;
    } else {
        return UseResult::Fail;
    }
    if !save(mob, s) {
        return UseResult::Fail;
    }
    if !click.creative {
        click.stack.shrink(1);
    }
    UseResult::Success
}

/// MossbackItem.use: preserve the baby bit, launch from the player's eye.
pub fn throw_item(click: &mut UseItem<'_>) -> UseResult {
    let (Some(player), Some(entities)) = (click.player, ENTITIES.get()) else {
        return UseResult::Fail;
    };
    let Some(view) = player.view() else {
        return UseResult::Fail;
    };
    if click.stack.count() == 0 {
        return UseResult::Fail;
    }
    let baby = click.stack.read().is_some_and(|data| captured_baby(&data));
    let (sy, cy) = f64::from(click.yaw).to_radians().sin_cos();
    let (sp, cp) = f64::from(click.pitch).to_radians().sin_cos();
    let look = [-sy * cp, -sp, cy * cp];
    let mut at = std::array::from_fn(|i| view.position[i] + look[i] * 0.65);
    // shortcut: player's eye position is not exposed; infer standing/sneaking height.
    at[1] += if click.sneaking() { 1.27 } else { 1.37 };
    let Some(id) = crate::roster::spawn(click.world, KIND, at) else {
        return UseResult::Fail;
    };
    let initialized = entities.with_entity(click.world, id, |mob| {
        if baby && !set_baby(mob) {
            mob.discard();
            return false;
        }
        let mut s = state(mob);
        s.thrown = true;
        let mut motion = look.map(|v| v * 1.25);
        motion[1] += 0.18;
        if !save(mob, s)
            || !mob.change(&EntityChange {
                mask: SET_MOTION | SET_YAW | moddedmc_api::entities::SET_ON_GROUND,
                motion,
                yaw: click.yaw,
                on_ground: false,
                ..Default::default()
            })
        {
            mob.discard();
            return false;
        }
        true
    }) == Some(true);
    if !initialized {
        return UseResult::Fail;
    }
    sound(
        click.world,
        view.position,
        "minecraft:entity.snowball.throw",
        "minecraft:entity.snowball.throw",
        0.6,
        0.85 + random_unit() as f32 * 0.25,
    );
    click.stack.shrink(1);
    UseResult::Success
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>()
}

fn mate(world: World<'_>, mob: Entity<'_>, at: [f64; 3]) -> Option<(i32, [f64; 3])> {
    let entities = ENTITIES.get()?;
    let mobs = MOBS.get()?;
    entities
        .entities_in(world, at.map(|v| v - 8.0), at.map(|v| v + 8.0), mob.id())
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |other| {
                    if mobs.kind(other) != Some(KIND) || !state(other).ready() {
                        return None;
                    }
                    Some((id, other.view()?.position))
                })
                .flatten()
        })
        .min_by(|a, b| distance(a.1, at).total_cmp(&distance(b.1, at)))
}

/// The released vanilla FloatGoal, PanicGoal, BreedGoal and WaterAvoidingRandomStrollGoal.
pub fn goal(phase: u32, key: u64, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(view)) = (MOB_AI.get(), mob.view()) else {
        return false;
    };
    let mut s = state(mob);
    if s.thrown {
        return false;
    }
    let wet = eggs::water_at(world, flight::cell(view.position));
    match key {
        FLOAT => match phase {
            goal_phase::CAN_START | goal_phase::CONTINUE => wet,
            goal_phase::TICK if ai::random(5) != 0 => {
                let mut motion = view.motion;
                motion[1] = motion[1].max(0.04);
                mob.change(&EntityChange {
                    mask: SET_MOTION,
                    motion,
                    ..Default::default()
                });
                false
            }
            _ => false,
        },
        PANIC => match phase {
            goal_phase::CAN_START => s.panic > 0,
            goal_phase::CONTINUE => s.panic > 0 && ai.navigating(mob),
            goal_phase::START => {
                // shortcut: vanilla panic seeks a random safe position (water when burning).
                let at = random_ground(world, view.position);
                ai.walk_to(mob, at, 1.3);
                false
            }
            goal_phase::STOP => {
                ai.stop(mob);
                false
            }
            _ => false,
        },
        BREED => match phase {
            goal_phase::CAN_START | goal_phase::CONTINUE => {
                s.ready() && mate(world, mob, view.position).is_some()
            }
            goal_phase::STOP => {
                s.courtship = 0;
                save(mob, s);
                ai.stop(mob);
                false
            }
            goal_phase::TICK if s.ready() => {
                if let Some((id, at)) = mate(world, mob, view.position) {
                    ai.look_at(mob, at);
                    ai.walk_to(mob, at, 1.0);
                    s.courtship += 1;
                    if s.courtship >= 60
                        && distance(at, view.position) < 9.0
                        && let Some(entities) = ENTITIES.get()
                        && let Some(child) = crate::roster::spawn(world, KIND, view.position)
                        && entities.with_entity(world, child, |baby| {
                            if set_baby(baby) {
                                true
                            } else {
                                baby.discard();
                                false
                            }
                        }) == Some(true)
                    {
                        s.age = COOLDOWN;
                        s.love = 0;
                        s.courtship = 0;
                        entities.with_entity(world, id, |other| {
                            let mut partner = state(other);
                            partner.age = COOLDOWN;
                            partner.love = 0;
                            partner.courtship = 0;
                            save(other, partner);
                        });
                        if let Some(mobs) = MOBS.get() {
                            mobs.tame_effect(world, mob, true);
                        }
                    } else if s.courtship >= 60 {
                        s.courtship = 0;
                    }
                    save(mob, s);
                }
                false
            }
            _ => false,
        },
        WANDER => match phase {
            goal_phase::CAN_START => ai::random(120) == 0,
            goal_phase::CONTINUE => ai.navigating(mob),
            goal_phase::START => {
                ai.walk_to(mob, random_ground(world, view.position), 0.8);
                false
            }
            goal_phase::STOP => {
                ai.stop(mob);
                false
            }
            _ => false,
        },
        _ => false,
    }
}

fn random_ground(world: World<'_>, origin: [f64; 3]) -> [f64; 3] {
    // shortcut: approximate vanilla land-position sampling; navigator rejects unreachable points.
    for _ in 0..10 {
        let at = [
            origin[0] + random_unit() * 20.0 - 10.0,
            origin[1],
            origin[2] + random_unit() * 20.0 - 10.0,
        ];
        if !eggs::water_at(world, flight::cell(at)) {
            return at;
        }
    }
    origin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_age_round_trip_and_growth() {
        assert!(captured_baby(&captured_stack(true)));
        assert!(!captured_baby(&captured_stack(false)));
        let mut s = State {
            age: -1,
            landed: 12,
            ..State::default()
        };
        assert!(s.tick());
        assert_eq!(s.age, 0);
        assert_eq!(s.landed, 11);
        assert!(!s.baby());
    }

    #[test]
    fn love_requires_an_adult_not_on_cooldown_or_thrown() {
        let mut s = State {
            love: LOVE_TICKS,
            ..State::default()
        };
        assert!(s.ready());
        s.thrown = true;
        assert!(!s.ready());
        s.thrown = false;
        s.age = COOLDOWN;
        assert!(!s.ready());
        s.age = 0;
        s.love = 1;
        s.tick();
        assert!(!s.ready());
    }
}
