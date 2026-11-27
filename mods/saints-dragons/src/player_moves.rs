//! DragonlordArmorSetBonus and BloodTempestArmorSetBonus (0.9.88).
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

use moddedmc_api::{
    Pos,
    abi::{
        PLAYER_INPUT_BACKWARD, PLAYER_INPUT_FORWARD, PLAYER_INPUT_JUMP, PLAYER_INPUT_LEFT,
        PLAYER_INPUT_RIGHT,
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

fn eligible(world: World<'_>, player: Entity<'_>, view: EntityView) -> bool {
    if player.flags() & (ENTITY_ALIVE | ENTITY_FLYING) != ENTITY_ALIVE
        || player.vehicle().is_some()
        || crate::MOBS.get().and_then(|m| m.game_mode(player)) == Some(3)
    {
        return false;
    }
    // shortcut: sample feet and torso for water/climbable; exact body-contact and
    // fall-flying state need player movement queries.
    for dy in [0.0, 0.9] {
        let pos = Pos {
            x: view.position[0].floor() as i32,
            y: (view.position[1] + dy).floor() as i32,
            z: view.position[2].floor() as i32,
        };
        if world
            .fluid_id(pos)
            .is_some_and(|id| matches!(id.as_str(), "minecraft:water" | "minecraft:flowing_water"))
        {
            return false;
        }
        let mut block = [0];
        if world.read(&[pos], &mut block)
            && crate::CONTENT
                .get()
                .and_then(|c| c.describe_state(block[0]))
                .is_some_and(|id| {
                    crate::TAGS.get().is_some_and(|tags| {
                        tags.contains(
                            "block",
                            "minecraft:climbable",
                            id.split('[').next().unwrap_or(&id),
                        )
                    })
                })
        {
            return false;
        }
    }
    true
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
    if !view.on_ground
        && state.airborne
        && state.peak.is_none()
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
    // shortcut: the next jump starts fall-flying in Java; host cannot set that state
    // or send the 40-51 tick client firework boost, so no fake flight is started.
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
            if !dragonlord && !tempest && states.get(&id).is_some_and(|s| now >= s.dodge_until) {
                states.remove(&id);
                return;
            }
            let s = state(&mut states, id, now);
            let pressed = input & !s.keys;
            s.keys = input;
            let can_move = eligible(world, player, view);
            if !can_move || !dragonlord {
                s.peak = None;
            }
            if !tempest {
                s.taps = [None; 4];
                s.immune_until = 0;
            }
            if player.flags() & ENTITY_ALIVE == 0 {
                states.remove(&id);
                return;
            }
            let mut landing = false;
            if dragonlord && can_move {
                if view.on_ground {
                    landing = s
                        .peak
                        .take()
                        .is_some_and(|peak| peak - view.position[1] >= 4.0);
                } else if let Some(peak) = s.peak.as_mut() {
                    *peak = peak.max(view.position[1]);
                } else if pressed & PLAYER_INPUT_JUMP != 0 {
                    try_jump(world, player, view, s);
                }
            }
            s.airborne = can_move && !view.on_ground;
            if tempest && can_move {
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
    let (Some(view), Some(armor)) = (
        player.view(),
        crate::EQUIPMENT.get().and_then(|e| e.worn_armor(player)),
    ) else {
        return;
    };
    if !eligible(world, player, view) {
        return;
    }
    let now = tick_count();
    let mut states = MOVEMENT.lock().unwrap_or_else(|e| e.into_inner());
    let s = state(&mut states, player.id(), now);
    if id == "saintsdragons:dragonlord_jump" && armor == crate::armor::DRAGONLORD {
        try_jump(world, player, view, s);
    } else if armor == crate::armor::BLOOD_TEMPEST {
        let direction = match id {
            "saintsdragons:blood_tempest_forward" => 0,
            "saintsdragons:blood_tempest_back" => 1,
            "saintsdragons:blood_tempest_left" => 2,
            "saintsdragons:blood_tempest_right" => 3,
            _ => return,
        };
        try_dodge(world, player, view, s, direction, now);
    }
    // shortcut: R sword abilities need an authoritative main-hand item query.
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
}
