//! `DragonHungerComponent` and the hunger-driven part of `DragonHappinessComponent` from 0.9.88.
//! No species overrides the base healing amount (0), so hunger never heals a dragon.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{content::World, entities::Entity, tick_count};
use serde::{Deserialize, Serialize};

use crate::{EFFECTS, ENTITIES, MOBS, taming};

pub const MAX: i32 = 100;
const DATA: &str = "saintsdragons:hunger";
/// The ambient goal ticks every tick; hunger runs once per this many ticks.
const STEP: i32 = 20;
const DECAY_INTERVAL: i32 = 4800;
const STARVE_INTERVAL: i32 = 80;
const STARVE_DAMAGE: f32 = 2.0;
const HAPPINESS_INTERVAL: i32 = 9600;
const HAPPINESS_DECAY: i32 = 2;
const HIT_PENALTY: i32 = 2;
const ANGRY_THRESHOLD: u8 = 60;

/// The species that carry `DragonEntity` care components.
pub const DRAGONS: [&str; 8] = [
    "cindervane",
    "raevyx",
    "stegonaut",
    "varasuchus",
    "ignivorus",
    "volitans",
    "nulljaw",
    "atroxiia",
];

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
struct Hunger {
    #[serde(rename = "Hunger")]
    hunger: i32,
    #[serde(rename = "HungerDecayTicks")]
    decay: i32,
    #[serde(rename = "HappinessDecayTicks")]
    happiness_decay: i32,
}

impl Default for Hunger {
    fn default() -> Self {
        Self {
            hunger: MAX,
            decay: 0,
            happiness_decay: 0,
        }
    }
}

impl Hunger {
    fn load(mob: Entity<'_>) -> Self {
        let mut hunger: Self = mob
            .data(DATA)
            .and_then(|nbt| fastnbt::from_bytes(&nbt).ok())
            .unwrap_or_default();
        hunger.hunger = hunger.hunger.clamp(0, MAX);
        hunger.decay = hunger.decay.max(0);
        hunger.happiness_decay = hunger.happiness_decay.max(0);
        hunger
    }

    fn save(self, mob: Entity<'_>) -> bool {
        fastnbt::to_bytes(&self).is_ok_and(|nbt| mob.set_data(DATA, &nbt))
    }
}

// The happiness each dragon's speed was last set for; keeps attribute writes to changes.
static APPLIED: Mutex<Option<HashMap<i32, u8>>> = Mutex::new(None);

pub fn level(mob: Entity<'_>) -> i32 {
    Hunger::load(mob).hunger
}

/// `applyFeedingHunger`: adds 20 for a hearty meal or 10 otherwise, and raises a tamed
/// dragon's happiness by 8 or 4. Returns whether the dragon was hungry.
pub fn feed(mob: Entity<'_>, hearty: bool) -> bool {
    let mut state = Hunger::load(mob);
    let hungry = state.hunger < MAX;
    state.hunger = (state.hunger + if hearty { 20 } else { 10 }).min(MAX);
    state.save(mob);
    taming::add_happiness(mob, if hearty { 8 } else { 4 });
    hungry
}

/// `getMeleeDamageMultiplier`.
pub fn melee_multiplier(mob: Entity<'_>) -> f32 {
    multiplier(level(mob))
}

fn multiplier(hunger: i32) -> f32 {
    match hunger {
        61.. => 1.0,
        ..=30 => 0.25,
        _ => 0.25 + 0.75 * (hunger - 30) as f32 / 30.0,
    }
}

/// `DragonHappinessComponent.getSpeedMultiplier`.
fn speed_multiplier(happiness: u8) -> f64 {
    match happiness {
        61.. => 1.0,
        ..=30 => 0.5,
        _ => 0.5 + 0.5 * f64::from(happiness - 30) / 30.0,
    }
}

fn ridden_by_player(world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) else {
        return false;
    };
    mobs.passengers(mob).first().is_some_and(|&rider| {
        entities.with_entity(world, rider, |e| e.type_id() == "minecraft:player") == Some(true)
    })
}

/// Hunger decay and starvation for every dragon, and happiness decay and its slowdown for a
/// tamed one. `speed` is the species' base ground speed and `flying` its base flying speed.
pub fn tick(world: World<'_>, mob: Entity<'_>, speed: f64, flying: Option<f64>) {
    if !(tick_count().wrapping_add(mob.id() as u64)).is_multiple_of(STEP as u64) {
        return;
    }
    let mut state = Hunger::load(mob);
    state.decay += STEP * if ridden_by_player(world, mob) { 2 } else { 1 };
    if state.hunger > 0 {
        while state.decay >= DECAY_INTERVAL && state.hunger > 0 {
            state.decay -= DECAY_INTERVAL;
            state.hunger -= 1;
        }
    } else {
        while state.decay >= STARVE_INTERVAL {
            state.decay -= STARVE_INTERVAL;
            mob.hurt("minecraft:starve", STARVE_DAMAGE);
        }
    }
    let happiness = if taming::is_tamed(mob) {
        state.happiness_decay += STEP
            * match state.hunger {
                ..=30 => 4,
                31..=60 => 2,
                _ => 1,
            };
        let mut drop = 0;
        while state.happiness_decay >= HAPPINESS_INTERVAL {
            state.happiness_decay -= HAPPINESS_INTERVAL;
            drop += HAPPINESS_DECAY;
        }
        if drop > 0 {
            taming::add_happiness(mob, -drop)
        } else {
            taming::happiness(mob)
        }
    } else {
        None
    };
    state.save(mob);
    // An untamed dragon runs at full speed (`clearSpeedModifiers`).
    apply_speed(mob, happiness.unwrap_or(100), speed, flying);
}

fn apply_speed(mob: Entity<'_>, happiness: u8, speed: f64, flying: Option<f64>) {
    let Some(health) = crate::MOB_HEALTH.get() else {
        return;
    };
    let mut applied = APPLIED.lock().unwrap_or_else(PoisonError::into_inner);
    let applied = applied.get_or_insert_default();
    let last = applied.insert(mob.id(), happiness);
    // A dragon seen for the first time at full happiness already has its base speed.
    if last.map_or(happiness == 100, |last| {
        speed_multiplier(last) == speed_multiplier(happiness)
    }) {
        return;
    }
    let multiplier = speed_multiplier(happiness);
    health.set_attribute(mob, "minecraft:movement_speed", speed * multiplier);
    if let Some(flying) = flying {
        health.set_attribute(mob, "minecraft:flying_speed", flying * multiplier);
    }
}

/// `applyHitPenalty`: a tamed dragon that took damage loses 2 happiness and sulks at 60 or less.
pub fn hit(world: World<'_>, mob: Entity<'_>) {
    let Some(happiness) = taming::add_happiness(mob, -HIT_PENALTY) else {
        return;
    };
    if happiness > ANGRY_THRESHOLD {
        return;
    }
    let (Some(effects), Some(view), Some(body)) = (EFFECTS.get(), mob.view(), mob.body()) else {
        return;
    };
    let [x, _, z] = view.position;
    effects.particles(
        world,
        "minecraft:angry_villager",
        [x, body.max[1] + 0.3, z],
        [0.3, 0.2, 0.3],
        0.0,
        6,
    );
}

pub fn forget(id: i32) {
    if let Some(applied) = APPLIED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        applied.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipliers_match_the_release() {
        assert_eq!(multiplier(100), 1.0);
        assert_eq!(multiplier(61), 1.0);
        assert_eq!(multiplier(45), 0.625);
        assert_eq!(multiplier(30), 0.25);
        assert_eq!(speed_multiplier(100), 1.0);
        assert_eq!(speed_multiplier(45), 0.75);
        assert_eq!(speed_multiplier(10), 0.5);
    }
}
