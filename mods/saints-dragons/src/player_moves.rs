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
        ENTITY_ALIVE, ENTITY_FLYING, ENTITY_LIVING, Entity, EntityChange, EntityView, SET_MOTION,
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
                && states
                    .get(&id)
                    .is_some_and(|s| !s.flight && now >= s.dodge_until && now >= s.boost_until)
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
            drop(states);
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

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}

fn scale(a: [f64; 3], n: f64) -> [f64; 3] {
    a.map(|v| v * n)
}

fn sword_target(player: Entity<'_>, target: Entity<'_>) -> bool {
    target.flags() & ENTITY_ALIVE != 0
        && target.is_attackable() == Some(true)
        && can_chain_from_hit(player, target)
}

/// SwordAbilityTargeting.canChainFromSuccessfulHit also accepts a dead primary.
fn can_chain_from_hit(player: Entity<'_>, target: Entity<'_>) -> bool {
    let Some(mobs) = crate::MOBS.get() else {
        return false;
    };
    target.id() != player.id()
        && !crate::taming::is_tamed(target)
        && target.view().is_some_and(|v| v.kind & ENTITY_LIVING != 0)
        && mobs.pet_owner(target).is_none()
        && !mobs.allied(player, target)
        && !mobs.allied(target, player)
}

pub fn successful_katana_hit(
    world: World<'_>,
    _item: &str,
    player: Entity<'_>,
    target: Entity<'_>,
    _damage: f32,
) {
    if can_chain_from_hit(player, target) {
        chain(world, player, target);
    }
}

fn chain_target(player: Entity<'_>, target: Entity<'_>) -> bool {
    sword_target(player, target)
        && !crate::TAGS.get().is_some_and(|tags| {
            let kind = crate::MOBS.get().and_then(|mobs| mobs.kind(target));
            let id = kind.unwrap_or_else(|| target.type_id());
            tags.contains("entity_type", "saintsdragons:immune_to/electricity", id)
        })
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

fn is_dragonlord_stack(bytes: &[u8]) -> bool {
    use fastnbt::Value;
    fastnbt::from_bytes::<HashMap<String, Value>>(bytes).is_ok_and(|stack| {
        matches!(stack.get("id"), Some(Value::String(id)) if id == crate::worldroot::DRAGONLORD_SWORD)
    })
}

fn sword_action(world: World<'_>, player: Entity<'_>) {
    let Some(slot) = player.player_selected_slot() else {
        return;
    };
    let (Some(view), Some(item), Some(dimension), Some(entities)) = (
        player.view(),
        player.player_inventory_item(slot),
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
        // DragonlordSwordAbility uses the released config defaults (enabled, 50 ticks).
        let stack = player.player_inventory_stack(slot);
        if stack.as_deref().is_some_and(is_dragonlord_stack) && player.player_item_ready(&item, 50)
        {
            crate::pillars::sword_wave(world, player, view.position, forward);
        }
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
                        if !chain_target(player, target) {
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
                if chain_target(player, first) {
                    first.hurt_by(player.id(), "minecraft:player_attack", damage);
                }
                crate::vfx::arc(world, add(at, [0.0, 0.35, 0.0]), at, true, 5, 1.0);
                if let Some(effects) = crate::EFFECTS.get() {
                    effects.particles(
                        world,
                        "minecraft:electric_spark",
                        add(at, [0.0, 0.175, 0.0]),
                        [0.06; 3],
                        0.0,
                        2,
                    );
                }
            }
            break;
        };
        entities.with_entity(world, id, |target| {
            let to = center(target);
            target.hurt_by(player.id(), "minecraft:player_attack", damage);
            if let Some(to) = to {
                crate::vfx::arc(world, at, to, true, 5, 1.0);
                if let Some(effects) = crate::EFFECTS.get() {
                    let midpoint = scale(add(at, to), 0.5);
                    effects.particles(
                        world,
                        "minecraft:electric_spark",
                        midpoint,
                        [0.06; 3],
                        0.0,
                        2,
                    );
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
            if !sword_target(player, target)
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
    fn only_dragonlord_main_hand_stack_activates_magma() {
        let stack = |id: &str| fastnbt::to_bytes(&HashMap::from([("id", id)])).unwrap();
        assert!(is_dragonlord_stack(&stack(
            "saintsdragons:dragonlord_sword"
        )));
        assert!(!is_dragonlord_stack(&stack(
            "saintsdragons:blood_tempest_katana"
        )));
        assert!(!is_dragonlord_stack(&[]));
    }
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
