//! ArrowOfVenomEntity from the 0.9.88 release.
//! shortcut: the projectile uses the vanilla arrow model until the companion renders projectile rigs.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, RegistrationError, Registry,
    entities::Entity,
    items::{self, ArrowAmmo},
};

use crate::MOBS;

pub const ID: &str = "saintsdragons:arrow_of_venom";
const VELOCITY_MULTIPLIER: f32 = 1.35;
const WATER_INERTIA: f64 = 1.0;
const POISON_TICKS: i32 = 30 * 20;
const BLINDNESS_TICKS: i32 = 5 * 20;

static AMMO: OnceLock<ArrowAmmo> = OnceLock::new();

pub fn start() -> Result<(), InterfaceUnavailable> {
    let _ = AMMO.set(items::require_arrow_ammo()?);
    Ok(())
}

pub fn register(reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    AMMO.get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .bind(reg, ID, VELOCITY_MULTIPLIER, WATER_INERTIA, after_hurt)
}

fn after_hurt(_item: &str, target: Entity<'_>) {
    if let Some(mobs) = MOBS.get() {
        mobs.reset_hurt_cooldown(target);
    }
    target.add_effect("minecraft:poison", POISON_TICKS, 1, false, true);
    target.add_effect("minecraft:blindness", BLINDNESS_TICKS, 0, false, true);
}
