//! Physical key bindings from DragonRideInputHandler in Saint's Dragons 0.9.88.
use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, RegistrationError, Registry,
    content::World,
    entities::Entity,
    player_actions::{self, ACTION_HELD, ACTION_PRESSED, ACTION_RELEASED, PlayerActions},
};

static ACTIONS: OnceLock<PlayerActions> = OnceLock::new();
const ALL: u8 = ACTION_PRESSED | ACTION_HELD | ACTION_RELEASED;
const KEYS: &[(&str, &str, u8)] = &[
    ("ascend", "key.keyboard.unknown", ALL),
    ("descend", "key.keyboard.left.alt", ALL),
    ("accelerate", "key.keyboard.unknown", ALL),
    ("ability_tertiary", "key.keyboard.g", ALL),
    ("ability_primary", "key.keyboard.r", ALL),
    ("ability_secondary", "key.keyboard.h", ALL),
    ("toggle_melee", "key.keyboard.x", ACTION_PRESSED),
    ("flex", "ctrl+key.keyboard.4", ACTION_PRESSED),
    // shortcut: independent rebindable key until the companion can observe the
    // vanilla attack mapping without stealing its clicks.
    ("attack", "key.keyboard.unknown", ACTION_PRESSED),
];

pub fn start() -> Result<(), InterfaceUnavailable> {
    let actions = player_actions::require()?;
    actions.require_mounted()?;
    let _ = ACTIONS.set(actions);
    Ok(())
}

pub fn register(reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let actions = ACTIONS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for &(name, key, phases) in KEYS {
        actions.add(
            reg,
            &format!("saintsdragons:{name}"),
            &format!("key.saintsdragons.{name}"),
            key,
            phases,
            rider_action,
        )?;
    }
    for id in [player_actions::MOUNTED_ATTACK, player_actions::MOUNTED_USE] {
        actions.subscribe(reg, id, ALL, rider_action)?;
    }
    for id in [
        player_actions::DOUBLE_TAP_FORWARD,
        player_actions::DOUBLE_TAP_BACK,
        player_actions::DOUBLE_TAP_LEFT,
        player_actions::DOUBLE_TAP_RIGHT,
    ] {
        actions.subscribe(reg, id, ACTION_PRESSED, rider_action)?;
    }
    Ok(())
}

fn rider_action(world: World<'_>, player: Entity<'_>, action: &str, phase: u8) {
    let Some(vehicle) = player.vehicle() else {
        return;
    };
    let Some(entities) = crate::ENTITIES.get() else {
        return;
    };
    let Some(mobs) = crate::MOBS.get() else {
        return;
    };
    // shortcut: only route mounted dragon input; individual ability implementations follow.
    let _ = entities.with_entity(world, vehicle, |dragon| {
        if mobs.kind(dragon) == Some("saintsdragons:raevyx") {
            crate::combat_raevyx::rider_action(world, dragon, player, action, phase);
            return;
        }
        if mobs.kind(dragon) == Some("saintsdragons:volitans") {
            crate::combat_volitans::rider_input(world, dragon, player, action, phase);
            return;
        }
        if mobs.kind(dragon) == Some("saintsdragons:varasuchus") {
            crate::combat_varasuchus::rider_action(world, dragon, player, action, phase);
            return;
        }
        if mobs.kind(dragon) == Some("saintsdragons:nulljaw") {
            crate::combat_nulljaw::rider_action(world, dragon, player, action, phase);
            return;
        }
        if mobs.kind(dragon) == Some("saintsdragons:stegonaut") {
            crate::combat_stegonaut::rider_action(world, dragon, player, action, phase);
            return;
        }
        if mobs.kind(dragon) == Some("saintsdragons:atroxiia") {
            crate::combat_atroxiia::rider_action(world, player, dragon, action, phase);
            return;
        }
        if mobs.kind(dragon) == Some("saintsdragons:ignivorus") {
            crate::combat_ignivorus::rider_action(world, dragon, player, action, phase);
            return;
        }
        if phase != ACTION_HELD
            && mobs
                .kind(dragon)
                .is_some_and(|kind| kind.starts_with("saintsdragons:"))
        {
            moddedmc_api::info(&format!(
                "saintsdragons: rider action {action} phase {phase}"
            ));
        }
    });
}
