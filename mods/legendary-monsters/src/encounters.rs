//! Native Ancient Stronghold encounters: Spiky Bug and Ambusher.
//! The stock client sees vanilla-compatible carriers, while target selection,
//! timed attack windows, immunities, persistence and death reinforcements are
//! handled natively. Original proprietary models/sounds are not redistributed.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Command, RegistrationError, Registry,
    commands::{Argument, Node},
    content::World,
    entities::{ENTITY_ALIVE, ENTITY_LIVING, ENTITY_PLAYER, Entity},
    mob_ai::{Goal, goal_control, goal_phase},
    mobs::Mob,
};

use crate::{COMMANDS, CONTENT, ENTITIES, MOB_AI, MOBS};

pub const SPIKY_BUG: &str = "legendary_monsters:spiky_bug";
pub const AMBUSHER: &str = "legendary_monsters:ambusher";
const TARGET: u64 = 1;
const ATTACK: u64 = 2;
const WANDER: u64 = 3;
const LOOK: u64 = 4;
const PERSIST: u64 = 5;
const FOLLOW_RANGE: f64 = 14.0;
const REACH_SQUARED: f64 = 8.75;
const ATTACK_DELAY: u8 = 8;
const MELEE_DAMAGE: f32 = 3.0;
const AMBUSHER_DAMAGE: f32 = 8.0;
const AMBUSHER_REACH_SQUARED: f64 = 16.0;
const AMBUSHER_HEIGHT: f64 = 3.0;
const AMBUSHER_ARC: f64 = 150.0;
const AMBUSHER_COOLDOWN: u8 = 48;
const SPAWN_COMMAND: u64 = 101;
const FIND_COMMAND: u64 = 102;
const AMBUSHER_SPAWN_COMMAND: u64 = 103;
const AMBUSHER_FIND_COMMAND: u64 = 104;

#[derive(Default)]
struct Chase {
    target_ticks: u32,
    candidates: usize,
    valid: usize,
    target: Option<i32>,
    retaliation_tick: Option<i32>,
    countdown: u8,
    repath: u8,
    wander: Option<[f64; 3]>,
}

static CHASE: Mutex<Option<HashMap<i32, Chase>>> = Mutex::new(None);

fn with_chase<T>(id: i32, action: impl FnOnce(&mut Chase) -> T) -> T {
    let mut all = CHASE.lock().unwrap_or_else(PoisonError::into_inner);
    action(all.get_or_insert_default().entry(id).or_default())
}

fn distance_squared(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// The released Ambusher attack is a 150-degree, four-block frontal cone
/// with a three-block vertical band. Normalize yaw around the wrap boundary,
/// otherwise targets near -180/+180 would incorrectly evade the attack.
fn ambusher_can_hit(from: [f64; 3], to: [f64; 3], facing: f32) -> bool {
    let dx = to[0] - from[0];
    let dz = to[2] - from[2];
    if dx * dx + dz * dz > AMBUSHER_REACH_SQUARED || (to[1] - from[1]).abs() > AMBUSHER_HEIGHT {
        return false;
    }
    let target_yaw = dz.atan2(dx).to_degrees() - 90.0;
    let angle = (target_yaw - f64::from(facing) + 180.0).rem_euclid(360.0) - 180.0;
    angle.abs() <= AMBUSHER_ARC / 2.0
}

fn valid_target(target: Entity<'_>) -> bool {
    let Some(view) = target.view() else {
        return false;
    };
    if target.flags() & ENTITY_ALIVE == 0 || view.kind & ENTITY_LIVING == 0 {
        return false;
    }
    if view.kind & ENTITY_PLAYER != 0 {
        return MOBS
            .get()
            .is_some_and(|mobs| !matches!(mobs.game_mode(target), Some(1 | 3) | None));
    }
    matches!(target.type_id(), "iron_golem" | "snow_golem")
}

fn target_position(world: World<'_>, id: i32) -> Option<[f64; 3]> {
    ENTITIES.get()?.with_entity(world, id, |entity| {
        valid_target(entity).then_some(entity.view()?.position)
    })?
}

fn select_target(world: World<'_>, mob: Entity<'_>) {
    let (Some(ai), Some(entities), Some(origin)) = (MOB_AI.get(), ENTITIES.get(), mob.view())
    else {
        return;
    };
    if let Some(current) = ai.target(mob) {
        if target_position(world, current).is_some_and(|position| {
            distance_squared(position, origin.position) <= FOLLOW_RANGE.powi(2)
        }) {
            return;
        }
        ai.set_target(world, mob, None);
    }

    if let Some((attacker, time)) = ai.last_hurt_by(mob) {
        let recent = with_chase(mob.id(), |chase| {
            if chase.retaliation_tick == Some(time) {
                false
            } else {
                chase.retaliation_tick = Some(time);
                true
            }
        });
        if recent && target_position(world, attacker).is_some() {
            ai.set_target(world, mob, Some(attacker));
            return;
        }
    }
    let min = origin.position.map(|coordinate| coordinate - FOLLOW_RANGE);
    let max = origin.position.map(|coordinate| coordinate + FOLLOW_RANGE);
    let nearby = entities.entities_matching(world, min, max, mob.id(), 0);
    let candidates = nearby.len();
    let mut valid = 0;
    let candidate = nearby
        .into_iter()
        .filter_map(|id| {
            let position = target_position(world, id)?;
            valid += 1;
            let distance = distance_squared(position, origin.position);
            (distance <= FOLLOW_RANGE.powi(2)).then_some((id, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    with_chase(mob.id(), |chase| {
        chase.candidates = candidates;
        chase.valid = valid;
    });
    if let Some((id, _)) = candidate {
        ai.set_target(world, mob, Some(id));
    }
}

fn attack(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let Some(ai) = MOB_AI.get() else { return false };
    let ambusher = MOBS.get().and_then(|mobs| mobs.kind(mob)) == Some(AMBUSHER);
    let candidate = ai
        .target(mob)
        .and_then(|id| Some((id, target_position(world, id)?)));
    let position = mob.view().map(|view| view.position);
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => candidate.is_some() && position.is_some(),
        goal_phase::START => {
            with_chase(mob.id(), |state| {
                state.countdown = ATTACK_DELAY;
                state.repath = 0;
                state.target = candidate.map(|(id, _)| id);
            });
            false
        }
        goal_phase::STOP => {
            ai.stop(mob);
            with_chase(mob.id(), |state| state.target = None);
            false
        }
        goal_phase::TICK => {
            let (Some((id, at)), Some(from)) = (candidate, position) else {
                return false;
            };
            ai.look_at(mob, at);
            let gap = distance_squared(from, at);
            if gap
                > if ambusher {
                    AMBUSHER_REACH_SQUARED
                } else {
                    REACH_SQUARED
                }
            {
                let repath = with_chase(mob.id(), |state| {
                    if state.repath > 0 {
                        state.repath -= 1;
                        false
                    } else {
                        state.repath = 4;
                        state.countdown = ATTACK_DELAY * 2;
                        true
                    }
                });
                if repath || !ai.navigating(mob) {
                    ai.walk_to(mob, at, 3.0);
                }
                return false;
            }
            ai.stop(mob);
            let ready = with_chase(mob.id(), |state| {
                if state.target != Some(id) {
                    state.countdown = ATTACK_DELAY;
                    state.target = Some(id);
                }
                state.countdown = state.countdown.saturating_sub(1);
                if state.countdown != 0 {
                    return false;
                }
                state.countdown = if ambusher {
                    AMBUSHER_COOLDOWN
                } else {
                    ATTACK_DELAY * 2
                };
                true
            });
            if ready && target_position(world, id).is_some() {
                let in_arc = !ambusher
                    || mob
                        .view()
                        .is_some_and(|view| ambusher_can_hit(from, at, view.yaw));
                if in_arc {
                    ai.attack(
                        world,
                        mob,
                        id,
                        if ambusher {
                            AMBUSHER_DAMAGE
                        } else {
                            MELEE_DAMAGE
                        },
                    );
                }
            }
            false
        }
        _ => false,
    }
}

fn wander(phase: u32, mob: Entity<'_>) -> bool {
    let Some(ai) = MOB_AI.get() else { return false };
    if ai.target(mob).is_some() {
        return false;
    }
    match phase {
        goal_phase::CAN_START => {
            let tick = moddedmc_api::tick_count();
            if (tick ^ mob.id() as u64).is_multiple_of(180) {
                if let Some(view) = mob.view() {
                    let x = ((tick.rotate_left(13) ^ mob.id() as u64) % 17) as f64 - 8.0;
                    let z = ((tick.rotate_left(29) ^ mob.id() as u64) % 17) as f64 - 8.0;
                    with_chase(mob.id(), |state| {
                        state.wander =
                            Some([view.position[0] + x, view.position[1], view.position[2] + z]);
                    });
                    return true;
                }
            }
            false
        }
        goal_phase::CONTINUE => ai.navigating(mob),
        goal_phase::START => {
            if let Some(at) = with_chase(mob.id(), |state| state.wander) {
                ai.walk_to(mob, at, 3.0);
            }
            false
        }
        goal_phase::STOP => {
            ai.stop(mob);
            with_chase(mob.id(), |state| state.wander = None);
            false
        }
        _ => false,
    }
}

fn goal(phase: u32, key: u64, world: World<'_>, mob: Entity<'_>) -> bool {
    match key {
        TARGET => {
            if phase == goal_phase::TICK {
                with_chase(mob.id(), |state| {
                    state.target_ticks = state.target_ticks.saturating_add(1)
                });
                select_target(world, mob);
            }
            matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE)
        }
        ATTACK => attack(phase, world, mob),
        WANDER => wander(phase, mob),
        LOOK => {
            if phase == goal_phase::TICK && MOB_AI.get().is_some_and(|ai| ai.target(mob).is_none())
            {
                if let Some(view) = mob.view() {
                    let offset = f64::from(view.yaw).to_radians();
                    MOB_AI.get().unwrap().look_at(
                        mob,
                        [
                            view.position[0] - offset.sin() * 3.0,
                            view.position[1] + 0.5,
                            view.position[2] + offset.cos() * 3.0,
                        ],
                    );
                }
            }
            matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE)
        }
        PERSIST => {
            if phase == goal_phase::TICK {
                if let Some(mobs) = MOBS.get() {
                    mobs.make_persistent(mob);
                }
            }
            matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE)
        }
        _ => false,
    }
}

fn hurt(_: World<'_>, _: Entity<'_>, kind: &str, _: f32) -> bool {
    kind != "minecraft:fall"
}

fn ambusher_hurt(_: World<'_>, _: Entity<'_>, kind: &str, _: f32) -> bool {
    // Released default allows projectile immunity (arrows), fall immunity,
    // and trident immunity. Other damage sources still reach the mob.
    !matches!(
        kind,
        "minecraft:fall" | "minecraft:trident" | "minecraft:arrow"
    )
}

fn death(_: World<'_>, mob: Entity<'_>) {
    CHASE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .remove(&mob.id());
}

fn ambusher_death(world: World<'_>, mob: Entity<'_>) {
    let position = mob.view().map(|view| view.position);
    death(world, mob);
    // The released die() spawns exactly three live Spiky Bugs at the death
    // location. These are separate saved entities, not a particle substitute.
    if let (Some(mobs), Some(at)) = (MOBS.get(), position) {
        for _ in 0..3 {
            if let Some(id) = mobs.spawn(world, SPIKY_BUG, at) {
                if let Some(entities) = ENTITIES.get() {
                    entities.with_entity(world, id, |bug| mobs.make_persistent(bug));
                }
            }
        }
    }
}

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let missing = RegistrationError::InvalidDefinition;
    let mobs = MOBS.get().ok_or(missing)?;
    mobs.add_mob(
        registry,
        &Mob {
            id: SPIKY_BUG,
            carrier: "minecraft:silverfish",
            attributes: &[
                ("minecraft:max_health", 7.0),
                ("minecraft:knockback_resistance", 0.1),
                ("minecraft:follow_range", FOLLOW_RANGE),
                ("minecraft:armor", 6.0),
                ("minecraft:movement_speed", 0.105),
                ("minecraft:attack_knockback", 0.5),
                ("minecraft:attack_damage", 3.0),
            ],
        },
    )?;
    mobs.on_death(registry, SPIKY_BUG, death)?;
    mobs.add_mob(
        registry,
        &Mob {
            id: AMBUSHER,
            // Stock-client visual/physics carrier. Its AI and combat are native,
            // not the spider's built-in goals; proprietary skin is unavailable.
            carrier: "minecraft:spider",
            attributes: &[
                ("minecraft:max_health", 50.0),
                ("minecraft:knockback_resistance", 0.5),
                ("minecraft:follow_range", FOLLOW_RANGE),
                ("minecraft:armor", 6.0),
                ("minecraft:movement_speed", 0.1),
                ("minecraft:attack_knockback", 0.5),
                ("minecraft:attack_damage", 8.0),
            ],
        },
    )?;
    mobs.on_death(registry, AMBUSHER, ambusher_death)?;
    let ai = MOB_AI.get().ok_or(missing)?;
    ai.on_goal(goal)?;
    ai.replace_goals(registry, SPIKY_BUG)?;
    ai.on_hurt(registry, SPIKY_BUG, hurt)?;
    ai.replace_goals(registry, AMBUSHER)?;
    ai.on_hurt(registry, AMBUSHER, ambusher_hurt)?;
    for (key, priority, controls) in [
        (TARGET, 1, goal_control::TARGET),
        (ATTACK, 1, goal_control::MOVE | goal_control::LOOK),
        (WANDER, 2, goal_control::MOVE),
        (LOOK, 3, goal_control::LOOK),
        (PERSIST, 0, 0),
    ] {
        for kind in [SPIKY_BUG, AMBUSHER] {
            ai.add_goal(
                registry,
                &Goal {
                    kind,
                    key,
                    priority,
                    controls,
                },
            )?;
        }
    }
    COMMANDS.get().ok_or(missing)?.add(
        registry,
        "legendary",
        "Operator commands for Legendary Monsters native encounters",
        2,
        &[
            Node {
                parent: None,
                name: "spiky",
                argument: Argument::Literal,
                tag: None,
            },
            Node {
                parent: Some(0),
                name: "spawn",
                argument: Argument::Literal,
                tag: None,
            },
            Node {
                parent: Some(1),
                name: "x",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: None,
            },
            Node {
                parent: Some(2),
                name: "y",
                argument: Argument::Integer { min: -64, max: 512 },
                tag: None,
            },
            Node {
                parent: Some(3),
                name: "z",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: Some(SPAWN_COMMAND),
            },
            Node {
                parent: Some(0),
                name: "find",
                argument: Argument::Literal,
                tag: None,
            },
            Node {
                parent: Some(5),
                name: "x",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: None,
            },
            Node {
                parent: Some(6),
                name: "y",
                argument: Argument::Integer { min: -64, max: 512 },
                tag: None,
            },
            Node {
                parent: Some(7),
                name: "z",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: Some(FIND_COMMAND),
            },
            Node {
                parent: None,
                name: "ambusher",
                argument: Argument::Literal,
                tag: None,
            },
            Node {
                parent: Some(9),
                name: "spawn",
                argument: Argument::Literal,
                tag: None,
            },
            Node {
                parent: Some(10),
                name: "x",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: None,
            },
            Node {
                parent: Some(11),
                name: "y",
                argument: Argument::Integer { min: -64, max: 512 },
                tag: None,
            },
            Node {
                parent: Some(12),
                name: "z",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: Some(AMBUSHER_SPAWN_COMMAND),
            },
            Node {
                parent: Some(9),
                name: "find",
                argument: Argument::Literal,
                tag: None,
            },
            Node {
                parent: Some(14),
                name: "x",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: None,
            },
            Node {
                parent: Some(15),
                name: "y",
                argument: Argument::Integer { min: -64, max: 512 },
                tag: None,
            },
            Node {
                parent: Some(16),
                name: "z",
                argument: Argument::Integer {
                    min: -30_000_000,
                    max: 30_000_000,
                },
                tag: Some(AMBUSHER_FIND_COMMAND),
            },
        ],
    )?;
    Ok(())
}

pub fn command(tag: u64, command: Command<'_>) -> i32 {
    let (Some(commands), Some(content), Some(mobs), Some(entities), Some(ai)) = (
        COMMANDS.get(),
        CONTENT.get(),
        MOBS.get(),
        ENTITIES.get(),
        MOB_AI.get(),
    ) else {
        return 0;
    };
    let (Some(x), Some(y), Some(z), Some(world)) = (
        commands.integer(&command, "x"),
        commands.integer(&command, "y"),
        commands.integer(&command, "z"),
        content.command_world(&command),
    ) else {
        return 0;
    };
    let at = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
    match tag {
        SPAWN_COMMAND | AMBUSHER_SPAWN_COMMAND => {
            let kind = if tag == AMBUSHER_SPAWN_COMMAND {
                AMBUSHER
            } else {
                SPIKY_BUG
            };
            let Some(id) = mobs.spawn(world, kind, at) else {
                command.reply("Legendary native encounter spawn failed (destination must be loaded and valid)");
                return 0;
            };
            entities.with_entity(world, id, |mob| {
                mobs.make_persistent(mob);
            });
            if kind == SPIKY_BUG {
                command.reply(&format!("Spiky Bug spawned: id={id}, kind={SPIKY_BUG}"));
            } else {
                command.reply(&format!("Ambusher spawned: id={id}, kind={AMBUSHER}"));
            }
            1
        }
        FIND_COMMAND | AMBUSHER_FIND_COMMAND => {
            let kind = if tag == AMBUSHER_FIND_COMMAND {
                AMBUSHER
            } else {
                SPIKY_BUG
            };
            let min = at.map(|coordinate| coordinate - 12.0);
            let max = at.map(|coordinate| coordinate + 12.0);
            let mut found = Vec::new();
            for id in entities.entities_matching(world, min, max, -1, 0) {
                if let Some(record) = entities.with_entity(world, id, |entity| {
                    (mobs.kind(entity) == Some(kind)).then(|| {
                        let (health, max) = mobs.health(entity).unwrap_or((0.0, 0.0));
                        let probe = with_chase(id, |chase| (chase.target_ticks, chase.candidates, chase.valid));
                        format!("id={id} health={health:.1}/{max:.1} target={:?} state={:?} target_ticks={} candidates={} valid={}",
                            ai.target(entity), mobs.state(entity), probe.0, probe.1, probe.2)
                    })
                }).flatten() {
                    found.push(record);
                }
            }
            found.sort();
            let label = if kind == SPIKY_BUG { "Spiky Bug" } else { "Ambusher" };
            command.reply(&format!("{label} found {}: {}", found.len(), found.join("; ")));
            found.len() as i32
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_attack_reach_and_timing() {
        assert_eq!(REACH_SQUARED, 3.5 * 2.5);
        assert_eq!(ATTACK_DELAY * 2, 16);
        assert_eq!(MELEE_DAMAGE, 3.0);
        assert!(distance_squared([0.0; 3], [2.0, 0.0, 2.0]) <= REACH_SQUARED);
        assert!(distance_squared([0.0; 3], [3.0, 0.0, 3.0]) > REACH_SQUARED);
    }

    #[test]
    fn ambusher_source_arc_height_reach_and_attack_cooldown() {
        assert_eq!(AMBUSHER_COOLDOWN, 48);
        assert_eq!(AMBUSHER_DAMAGE, 8.0);
        assert!(ambusher_can_hit([0.0; 3], [0.0, 0.0, 3.0], 0.0));
        assert!(!ambusher_can_hit([0.0; 3], [0.0, 0.0, -3.0], 0.0));
        assert!(!ambusher_can_hit([0.0; 3], [0.0, 4.0, 3.0], 0.0));
        assert!(!ambusher_can_hit([0.0; 3], [0.0, 0.0, 5.0], 0.0));
    }
}
