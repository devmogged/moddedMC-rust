//! DragonlordArmorSetBonus and BloodTempestArmorSetBonus (0.9.88).
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

use moddedmc_api::{
    abi::{
        ENTITY_IN_WATER, ENTITY_ON_CLIMBABLE, PLAYER_INPUT_BACKWARD, PLAYER_INPUT_FORWARD,
        PLAYER_INPUT_JUMP, PLAYER_INPUT_LEFT, PLAYER_INPUT_RIGHT,
    },
    content::World,
    effects::SOUND_PLAYERS,
    entities::{
        ENTITY_ALIVE, ENTITY_FLYING, ENTITY_LIVING, ENTITY_PLAYER, Entity, EntityChange,
        EntityView, SET_MOTION,
    },
    tick_count,
};

const DIRECTIONS: [i32; 4] = [
    PLAYER_INPUT_FORWARD,
    PLAYER_INPUT_BACKWARD,
    PLAYER_INPUT_LEFT,
    PLAYER_INPUT_RIGHT,
];
const JUMP: f64 = 0.85;
const DODGE_SPEED: f64 = 4.0;
const DODGE_COOLDOWN: u64 = 30;
const DODGE_IFRAMES: u64 = 8;
const TAP_WINDOW: u64 = 4;

#[derive(Default)]
struct Movement {
    last_seen: u64,
    keys: i32,
    airborne: bool,
    peak: Option<f64>,
    flight: bool,
    boost_until: u64,
    pillars: Vec<Pillar>,
    taps: [Option<u64>; 4],
    dodge_until: u64,
    immune_until: u64,
}

static MOVEMENT: LazyLock<Mutex<HashMap<i32, Movement>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn state(states: &mut HashMap<i32, Movement>, id: i32, now: u64) -> &mut Movement {
    let state = states.entry(id).or_default();
    if now.saturating_sub(state.last_seen) > 1 {
        *state = Movement::default();
    }
    state.last_seen = now;
    state
}

fn eligible(player: Entity<'_>) -> bool {
    player.flags() & (ENTITY_ALIVE | ENTITY_FLYING | ENTITY_IN_WATER | ENTITY_ON_CLIMBABLE)
        == ENTITY_ALIVE
        && player.vehicle().is_none()
        && crate::MOBS.get().and_then(|m| m.game_mode(player)) != Some(3)
}

fn motion(player: Entity<'_>, velocity: [f64; 3]) -> bool {
    player.change(&EntityChange {
        mask: SET_MOTION,
        motion: velocity,
        ..Default::default()
    })
}

fn sound(world: World<'_>, at: [f64; 3], id: &str, volume: f32, pitch: f32) {
    if let Some(effects) = crate::EFFECTS.get() {
        effects.play_sound(world, at, id, SOUND_PLAYERS, volume, pitch);
    }
}

fn jump_velocity(mut velocity: [f64; 3]) -> [f64; 3] {
    velocity[1] = JUMP.max(velocity[1] + JUMP);
    velocity
}

fn dodge_velocity(yaw: f32, direction: usize, y: f64) -> [f64; 3] {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    let [x, z] = match direction {
        0 => [-sin, cos],
        1 => [sin, -cos],
        2 => [cos, sin],
        _ => [-cos, -sin],
    };
    [x * DODGE_SPEED, y, z * DODGE_SPEED]
}

fn double_tap(last: &mut Option<u64>, now: u64) -> bool {
    if *last == Some(now) {
        return false;
    }
    if last.is_some_and(|then| now.saturating_sub(then) <= TAP_WINDOW) {
        *last = None;
        true
    } else {
        *last = Some(now);
        false
    }
}

fn try_jump(world: World<'_>, player: Entity<'_>, view: EntityView, state: &mut Movement) {
    if view.on_ground {
        return;
    }
    if state.flight || state.peak.is_some() {
        if !state.flight {
            if !player.set_player_fall_flying(true) {
                return;
            }
            state.flight = true;
            state.peak = None;
            player.reset_fall_distance();
        }
        let now = tick_count();
        if now >= state.boost_until {
            let duration = 40 + crate::ai::random(6) as u32 + crate::ai::random(7) as u32;
            player.player_flight_boost(duration);
            state.boost_until = now + 80;
            sound(
                world,
                view.position,
                "saintsdragons:dragonlord_armor_flap",
                0.9,
                0.96 + crate::ai::random(81) as f32 / 1000.0,
            );
        }
        return;
    }
    if !view.on_ground
        && state.airborne
        && state.peak.is_none()
        && player.player_fall_flying() == Some(false)
        && motion(player, jump_velocity(view.motion))
    {
        player.reset_fall_distance();
        state.peak = Some(view.position[1]);
        sound(
            world,
            view.position,
            "saintsdragons:dragonlord_armor_double_jump",
            0.75,
            1.0,
        );
    }
}

fn stop_flight(player: Entity<'_>, state: &mut Movement) {
    if state.flight {
        player.set_player_fall_flying(false);
        player.player_flight_boost(0);
        state.flight = false;
    }
}

fn try_dodge(
    world: World<'_>,
    player: Entity<'_>,
    view: EntityView,
    state: &mut Movement,
    direction: usize,
    now: u64,
) {
    if double_tap(&mut state.taps[direction], now)
        && now >= state.dodge_until
        && motion(player, dodge_velocity(view.yaw, direction, view.motion[1]))
    {
        state.dodge_until = now + DODGE_COOLDOWN;
        state.immune_until = now + DODGE_IFRAMES;
        sound(
            world,
            view.position,
            "saintsdragons:blood_tempest_armor_dodge",
            0.8,
            1.1,
        );
    }
}

pub fn tick(world: World<'_>, now: u64) {
    let (Some(entities), Some(equipment), Some(mobs)) = (
        crate::ENTITIES.get(),
        crate::EQUIPMENT.get(),
        crate::MOBS.get(),
    ) else {
        return;
    };
    for id in entities.players(world) {
        entities.with_entity(world, id, |player| {
            let Some(view) = player.view() else { return };
            let Some(armor) = equipment.worn_armor(player) else {
                return;
            };
            let dragonlord = armor == crate::armor::DRAGONLORD;
            let tempest = armor == crate::armor::BLOOD_TEMPEST;
            let input = mobs.player_input(player).unwrap_or(0);
            let mut states = MOVEMENT.lock().unwrap_or_else(|e| e.into_inner());
            if !dragonlord && !tempest && !states.contains_key(&id) {
                return;
            }
            if !dragonlord
                && !tempest
                && states.get(&id).is_some_and(|s| {
                    !s.flight
                        && s.pillars.is_empty()
                        && now >= s.dodge_until
                        && now >= s.boost_until
                })
            {
                states.remove(&id);
                return;
            }
            let s = state(&mut states, id, now);
            let pressed = input & !s.keys;
            s.keys = input;
            let can_move = eligible(player);
            if !can_move || !dragonlord {
                stop_flight(player, s);
                s.peak = None;
            }
            if !tempest {
                s.taps = [None; 4];
                s.immune_until = 0;
            }
            if player.flags() & ENTITY_ALIVE == 0 {
                stop_flight(player, s);
                states.remove(&id);
                return;
            }
            let mut landing = false;
            if dragonlord && can_move {
                if view.on_ground {
                    stop_flight(player, s);
                    landing = s
                        .peak
                        .take()
                        .is_some_and(|peak| peak - view.position[1] >= 4.0);
                } else {
                    if s.flight {
                        player.set_player_fall_flying(true);
                    }
                    if let Some(peak) = s.peak.as_mut() {
                        *peak = peak.max(view.position[1]);
                    }
                    if pressed & PLAYER_INPUT_JUMP != 0 {
                        try_jump(world, player, view, s);
                    }
                }
            }
            s.airborne = can_move && !view.on_ground;
            if tempest && can_move && player.player_fall_flying() == Some(false) {
                for (direction, key) in DIRECTIONS.into_iter().enumerate() {
                    if pressed & key != 0 {
                        try_dodge(world, player, view, s, direction, now);
                    }
                }
            } else {
                s.taps = [None; 4];
            }
            let pillars = std::mem::take(&mut s.pillars);
            drop(states);
            let pillars = tick_pillars(world, player, pillars, now);
            if !pillars.is_empty()
                && let Some(s) = MOVEMENT
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_mut(&id)
            {
                s.pillars.extend(pillars);
            }
            if landing {
                shockwave(world, player, view.position);
            }
        });
    }
    if now.is_multiple_of(20) {
        MOVEMENT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, s| now.saturating_sub(s.last_seen) <= 1);
    }
}

/// Optional named bindings use the same state as vanilla movement, avoiding a second
/// default mapping on Space/WASD. The shared primary (R) is reserved for both swords.
pub fn action(world: World<'_>, player: Entity<'_>, id: &str) {
    if id == "saintsdragons:ability_primary" {
        sword_action(world, player);
        return;
    }
    let (Some(view), Some(armor)) = (
        player.view(),
        crate::EQUIPMENT.get().and_then(|e| e.worn_armor(player)),
    ) else {
        return;
    };
    if !eligible(player) {
        return;
    }
    let now = tick_count();
    let mut states = MOVEMENT.lock().unwrap_or_else(|e| e.into_inner());
    let s = state(&mut states, player.id(), now);
    if id == "saintsdragons:dragonlord_jump" && armor == crate::armor::DRAGONLORD {
        try_jump(world, player, view, s);
    } else if armor == crate::armor::BLOOD_TEMPEST && player.player_fall_flying() == Some(false) {
        let direction = match id {
            "saintsdragons:blood_tempest_forward" => 0,
            "saintsdragons:blood_tempest_back" => 1,
            "saintsdragons:blood_tempest_left" => 2,
            "saintsdragons:blood_tempest_right" => 3,
            _ => return,
        };
        try_dodge(world, player, view, s, direction, now);
    }
}

struct Pillar {
    at: [f64; 3],
    stage: u32,
    start: u64,
    dimension: &'static str,
    seen: Vec<i32>,
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}

fn scale(a: [f64; 3], n: f64) -> [f64; 3] {
    a.map(|v| v * n)
}

fn sword_target(player: Entity<'_>, target: Entity<'_>) -> bool {
    // shortcut: the pets-seats host extension will distinguish owned vanilla
    // animals and scoreboard allies; until then protect all pet species.
    target.id() != player.id()
        && target.flags() & ENTITY_ALIVE != 0
        && !matches!(
            crate::MOBS.get().and_then(|m| m.game_mode(target)),
            Some(1 | 3)
        )
        && !crate::taming::is_tamed(target)
        && target.view().is_some_and(|v| v.kind & ENTITY_LIVING != 0)
        && !matches!(
            target.type_id().trim_start_matches("minecraft:"),
            "wolf"
                | "cat"
                | "parrot"
                | "horse"
                | "donkey"
                | "mule"
                | "llama"
                | "trader_llama"
                | "skeleton_horse"
                | "zombie_horse"
        )
}

fn center(target: Entity<'_>) -> Option<[f64; 3]> {
    let body = target.body()?;
    Some(std::array::from_fn(|i| (body.min[i] + body.max[i]) * 0.5))
}

fn dash_distance(mut clear: impl FnMut(f64) -> bool) -> f64 {
    let mut distance = 0.0;
    for step in 1..=80 {
        let d = f64::from(step) * 0.25;
        if !clear(d) {
            break;
        }
        distance = d;
    }
    distance
}

fn sword_action(world: World<'_>, player: Entity<'_>) {
    let (Some(view), Some(item), Some(dimension), Some(entities)) = (
        player.view(),
        player
            .player_selected_slot()
            .and_then(|slot| player.player_inventory_item(slot)),
        world.dimension_id(),
        crate::ENTITIES.get(),
    ) else {
        return;
    };
    if player.flags() & ENTITY_ALIVE == 0
        || crate::MOBS.get().and_then(|m| m.game_mode(player)) == Some(3)
        || !player.player_item_ready(&item, 0)
        || f64::from(view.pitch).to_radians().cos().powi(2) < 1e-4
    {
        return;
    }
    let (sin, cos) = f64::from(view.yaw).to_radians().sin_cos();
    let forward = [-sin, 0.0, cos];
    if item == "saintsdragons:dragonlord_sword" {
        if !player.player_item_ready(&item, 50) {
            return;
        }
        let now = tick_count();
        let mut pillars = Vec::new();
        for stage in 0..3 {
            let mut at = add(view.position, scale(forward, 3.0 + 6.0 * f64::from(stage)));
            let Some(y) = world.surface_height(
                at[0].floor() as i32,
                at[2].floor() as i32,
                moddedmc_api::abi::heightmap::MOTION_BLOCKING_NO_LEAVES,
            ) else {
                continue;
            };
            at[1] = f64::from(y);
            sound(
                world,
                at,
                "saintsdragons:ignivorus_magma_pillar",
                1.2,
                0.9 + crate::ai::random(201) as f32 / 1000.0,
            );
            pillars.push(Pillar {
                at,
                stage,
                start: now,
                dimension,
                seen: Vec::new(),
            });
        }
        state(
            &mut MOVEMENT.lock().unwrap_or_else(|e| e.into_inner()),
            player.id(),
            now,
        )
        .pillars
        .extend(pillars);
    } else if item == "saintsdragons:blood_tempest_katana"
        && eligible(player)
        && player.player_fall_flying() == Some(false)
    {
        let (Some(body), Some(handle), Some(attack)) = (
            player.body(),
            player.as_player(),
            player.attribute_value("minecraft:attack_damage"),
        ) else {
            return;
        };
        let distance = dash_distance(|d| player.can_move_by(scale(forward, d)));
        if distance < 0.75 {
            return;
        }
        let delta = scale(forward, distance);
        let destination = add(view.position, delta);
        let min = std::array::from_fn(|i| body.min[i] + delta[i].min(0.0) - 1.0);
        let max = std::array::from_fn(|i| body.max[i] + delta[i].max(0.0) + 1.0);
        let eye = [view.position[0], body.max[1] - 0.18, view.position[2]];
        let targets: Vec<_> = entities
            .entities_in(world, min, max, player.id())
            .into_iter()
            .filter(|&id| {
                entities
                    .with_entity(world, id, |target| {
                        sword_target(player, target)
                            && center(target).is_some_and(|to| {
                                world
                                    .raycast(eye, to, Some(player.id()))
                                    .is_some_and(|(block, _)| block.is_none())
                            })
                    })
                    .unwrap_or(false)
            })
            .collect();
        if !player.player_item_ready(&item, 60) {
            return;
        }
        if !handle.teleport(
            dimension,
            destination[0],
            destination[1],
            destination[2],
            None,
        ) {
            return;
        }
        player.player_reset_attack();
        motion(player, [0.0; 3]);
        let mut chained = false;
        for id in targets {
            entities.with_entity(world, id, |target| {
                if target.hurt_by(
                    player.id(),
                    "minecraft:player_attack",
                    (attack * 2.65) as f32,
                ) == Some(true)
                {
                    if !chained {
                        chained = true;
                        chain(world, player, target);
                    }
                    if let Some(v) = target.view() {
                        let resistance = target
                            .attribute_value("minecraft:knockback_resistance")
                            .unwrap_or(0.0);
                        let strength = 0.25 * (1.0 - resistance.clamp(0.0, 1.0));
                        if strength > 0.0 {
                            motion(
                                target,
                                [
                                    v.motion[0] * 0.5 + forward[0] * strength,
                                    if v.on_ground {
                                        (v.motion[1] * 0.5 + strength).min(0.4)
                                    } else {
                                        v.motion[1]
                                    },
                                    v.motion[2] * 0.5 + forward[2] * strength,
                                ],
                            );
                        }
                    }
                    if player
                        .player_selected_slot()
                        .and_then(|slot| player.player_inventory_item(slot))
                        .as_deref()
                        == Some(item.as_str())
                    {
                        player.player_damage_held_item(1);
                    }
                }
            });
        }
        sound(
            world,
            destination,
            "saintsdragons:blood_tempest_armor_ability",
            1.0,
            1.0,
        );
    }
}

fn tick_pillars(
    world: World<'_>,
    player: Entity<'_>,
    mut pillars: Vec<Pillar>,
    now: u64,
) -> Vec<Pillar> {
    let Some(entities) = crate::ENTITIES.get() else {
        return Vec::new();
    };
    pillars.retain_mut(|p| {
        let age = now.saturating_sub(p.start);
        if age > 43 || world.dimension_id() != Some(p.dimension) {
            return false;
        }
        if !(5..=34).contains(&age) {
            return true;
        }
        let stage = f64::from(p.stage);
        let size = 1.0 + stage * 0.2;
        let radius = 2.25 + size * 0.65;
        let min = add(p.at, [-radius, -0.75, -radius]);
        let max = add(p.at, [radius, 5.5 + size * 2.4 - 0.75, radius]);
        for id in entities.entities_in(world, min, max, player.id()) {
            if p.seen.contains(&id) {
                continue;
            }
            entities.with_entity(world, id, |target| {
                let Some(v) = target.view() else { return };
                if !sword_target(player, target) {
                    return;
                }
                if p.seen.is_empty() {
                    sound(
                        world,
                        p.at,
                        "saintsdragons:ignivorus_magma_pillar",
                        1.4,
                        0.8 + crate::ai::random(201) as f32 / 1000.0,
                    );
                }
                p.seen.push(id);
                target.hurt_by(
                    player.id(),
                    "minecraft:player_attack",
                    18.0 + p.stage as f32 * 4.0,
                );
                if let Some(mobs) = crate::MOBS.get() {
                    mobs.set_on_fire(target, 80);
                }
                let (mut x, mut z) = (v.position[0] - p.at[0], v.position[2] - p.at[2]);
                if x * x + z * z < 1e-4 {
                    x = 0.0;
                    z = 1.0;
                }
                let push = (0.9 + stage * 0.2) / x.hypot(z);
                motion(
                    target,
                    add(v.motion, [x * push, 0.35 + stage * 0.05, z * push]),
                );
            });
        }
        if let Some(effects) = crate::EFFECTS.get() {
            effects.particles(
                world,
                "minecraft:flame",
                add(p.at, [0.0, 2.0, 0.0]),
                [1.0, 2.0, 1.0],
                0.05,
                8,
            );
        }
        true
    });
    pillars
}

fn chain(world: World<'_>, player: Entity<'_>, first: Entity<'_>) {
    let (Some(entities), Some(mut at)) = (crate::ENTITIES.get(), center(first)) else {
        return;
    };
    let mut seen = vec![first.id()];
    let mut damage = 10.0;
    for jump in 0..5 {
        let nearest = entities
            .entities_in(world, add(at, [-8.0; 3]), add(at, [8.0; 3]), player.id())
            .into_iter()
            .filter(|id| !seen.contains(id))
            .filter_map(|id| {
                entities
                    .with_entity(world, id, |target| {
                        if !sword_target(player, target)
                            || crate::MOBS
                                .get()
                                .and_then(|m| m.kind(target))
                                .is_some_and(|k| k == "saintsdragons:raevyx")
                        {
                            return None;
                        }
                        let body = target.body()?;
                        let d: f64 = (0..3)
                            .map(|i| (at[i] - at[i].clamp(body.min[i], body.max[i])).powi(2))
                            .sum();
                        (d <= 64.0).then_some((id, d))
                    })
                    .flatten()
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((id, _)) = nearest else {
            if jump == 0 {
                first.hurt_by(player.id(), "minecraft:player_attack", damage);
            }
            break;
        };
        entities.with_entity(world, id, |target| {
            target.hurt_by(player.id(), "minecraft:player_attack", damage);
            if let Some(to) = center(target) {
                if let Some(effects) = crate::EFFECTS.get() {
                    effects.particles(world, "minecraft:electric_spark", to, [0.3; 3], 0.1, 16);
                }
                at = to;
            }
        });
        seen.push(id);
        damage *= 0.75;
    }
}

pub fn dodge_blocks(player: Entity<'_>, damage_type: &str) -> bool {
    let bypasses = crate::TAGS.get().is_some_and(|t| {
        t.contains(
            "damage_type",
            "minecraft:bypasses_invulnerability",
            damage_type,
        )
    }) || matches!(
        damage_type,
        "minecraft:out_of_world" | "minecraft:generic_kill"
    );
    !bypasses
        && MOVEMENT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&player.id())
            .is_some_and(|s| tick_count() < s.immune_until)
}

fn shockwave(world: World<'_>, player: Entity<'_>, at: [f64; 3]) {
    let Some(entities) = crate::ENTITIES.get() else {
        return;
    };
    for id in entities.entities_in(
        world,
        [at[0] - 7.5, at[1] - 5.0, at[2] - 7.5],
        [at[0] + 7.5, at[1] + 5.0, at[2] + 7.5],
        player.id(),
    ) {
        entities.with_entity(world, id, |target| {
            let Some(view) = target.view() else { return };
            // shortcut: cannot query vanilla ownership or team alliances. Restrict damage to
            // untamed mod dragons and non-player vanilla monsters until that query exists.
            let mod_dragon = crate::MOBS
                .get()
                .and_then(|m| m.kind(target))
                .is_some_and(|kind| kind.starts_with("saintsdragons:") && !kind.contains("ivy"));
            let kind = target.type_id();
            let kind = if kind.contains(':') {
                kind.to_owned()
            } else {
                format!("minecraft:{kind}")
            };
            let monster = crate::TAGS
                .get()
                .is_some_and(|t| t.contains("entity_type", "minecraft:undead", &kind))
                && !matches!(
                    kind.as_str(),
                    "minecraft:skeleton_horse" | "minecraft:zombie_horse"
                );
            if view.kind & ENTITY_LIVING == 0
                || view.kind & ENTITY_PLAYER != 0
                || target.flags() & ENTITY_ALIVE == 0
                || crate::taming::is_tamed(target)
                || !(mod_dragon || monster)
                || (view.position[0] - at[0]).powi(2) + (view.position[2] - at[2]).powi(2)
                    > 7.5_f64.powi(2)
            {
                return;
            }
            target.hurt_by(player.id(), "minecraft:player_attack", 20.0);
            let mut velocity = view.motion;
            velocity[1] += 0.75;
            motion(target, velocity);
        });
    }
    sound(
        world,
        at,
        "saintsdragons:dragonlord_armor_impact",
        1.35,
        0.92,
    );
    // shortcut: lava fissure, debris and camera effects require their companion/host carriers.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jump_preserves_horizontal_motion_and_adds_to_upward_velocity() {
        assert_eq!(jump_velocity([2.0, -1.0, -3.0]), [2.0, 0.85, -3.0]);
        assert_eq!(jump_velocity([0.0, 0.5, 0.0])[1], 1.35);
    }
    #[test]
    fn double_tap_window_is_inclusive_and_resets() {
        let mut last = None;
        assert!(!double_tap(&mut last, 10));
        assert!(!double_tap(&mut last, 10));
        assert!(double_tap(&mut last, 14));
        assert!(!double_tap(&mut last, 15));
        assert!(!double_tap(&mut last, 20));
    }
    #[test]
    fn dodge_uses_yaw_and_keeps_vertical_motion() {
        assert_eq!(dodge_velocity(0.0, 0, -0.2), [-0.0, -0.2, 4.0]);
        assert_eq!(dodge_velocity(0.0, 2, 0.2), [4.0, 0.2, 0.0]);
        assert!((dodge_velocity(90.0, 0, 0.0)[0] + 4.0).abs() < 1e-9);
    }

    #[test]
    fn katana_dash_stops_at_first_blocked_quarter_step_and_caps_at_twenty() {
        assert_eq!(dash_distance(|_| true), 20.0);
        assert_eq!(dash_distance(|d| d != 3.25), 3.0);
        assert_eq!(dash_distance(|d| d < 0.75), 0.5);
    }
}
