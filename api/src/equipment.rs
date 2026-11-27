//! Worn-armor and held-weapon hooks: change or cancel damage a player takes or deals.
use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, RegistrationError, Registry, abi, content::World, entities::Entity,
    guard, require_interface, s,
};

#[derive(Clone, Copy)]
pub struct Equipment(&'static abi::EquipmentV1);

/// The damage a player wearing `item` takes. `armor` is the head, chest, legs and feet item ids.
/// Return a negative value to cancel the hit.
pub type WornHurt = fn(
    world: World<'_>,
    item: &str,
    wearer: Entity<'_>,
    armor: [&str; 4],
    damage_type: &str,
    amount: f32,
    fall_distance: f32,
) -> f32;

/// The melee damage a player holding `item` deals to `target`, before its defenses.
pub type WeaponHit = fn(
    world: World<'_>,
    item: &str,
    attacker: Entity<'_>,
    target: Entity<'_>,
    damage_type: &str,
    amount: f32,
) -> f32;

static WORN: OnceLock<WornHurt> = OnceLock::new();
static WEAPON: OnceLock<WeaponHit> = OnceLock::new();

pub fn require() -> Result<Equipment, InterfaceUnavailable> {
    // SAFETY: EquipmentV1 is the published table type for moddedmc:equipment v1.
    unsafe { require_interface(abi::EQUIPMENT, 1) }.map(Equipment)
}

/// One handler per mod for each hook; it receives the item id.
fn same<F: Copy>(slot: &OnceLock<F>, handler: F, eq: fn(F, F) -> bool) -> bool {
    eq(*slot.get_or_init(|| handler), handler)
}

impl Equipment {
    pub fn on_worn_hurt(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        handler: WornHurt,
    ) -> Result<(), RegistrationError> {
        if !same(&WORN, handler, std::ptr::fn_addr_eq) {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: the registration and item id are borrowed and the trampoline is static.
        RegistrationError::from_code(unsafe { (self.0.on_worn_hurt)(reg.reg, s(item), worn) })
    }

    pub fn on_weapon_hit(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        handler: WeaponHit,
    ) -> Result<(), RegistrationError> {
        if !same(&WEAPON, handler, std::ptr::fn_addr_eq) {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: the registration and item id are borrowed and the trampoline is static.
        RegistrationError::from_code(unsafe { (self.0.on_weapon_hit)(reg.reg, s(item), weapon) })
    }
}

unsafe extern "C" fn worn(
    world: abi::WorldRef,
    item: abi::Str,
    wearer: abi::EntityRef,
    armor: *const [abi::Str; 4],
    damage_type: abi::Str,
    amount: f32,
    fall_distance: f32,
) -> f32 {
    guard("worn item hurt", amount, || {
        // SAFETY: the host borrows the four ids for this call.
        let (Some(handler), Some(world), Some(wearer), Some(armor)) = (
            WORN.get(),
            World::borrowed(world),
            Entity::borrowed(wearer),
            unsafe { armor.as_ref() },
        ) else {
            return amount;
        };
        // SAFETY: the host borrows every id for this call.
        let (item, damage_type, armor) = unsafe {
            (
                item.str(),
                damage_type.str(),
                [
                    armor[0].str(),
                    armor[1].str(),
                    armor[2].str(),
                    armor[3].str(),
                ],
            )
        };
        handler(
            world,
            item,
            wearer,
            armor,
            damage_type,
            amount,
            fall_distance,
        )
    })
}

unsafe extern "C" fn weapon(
    world: abi::WorldRef,
    item: abi::Str,
    attacker: abi::EntityRef,
    target: abi::EntityRef,
    damage_type: abi::Str,
    amount: f32,
) -> f32 {
    guard("weapon hit", amount, || {
        let (Some(handler), Some(world), Some(attacker), Some(target)) = (
            WEAPON.get(),
            World::borrowed(world),
            Entity::borrowed(attacker),
            Entity::borrowed(target),
        ) else {
            return amount;
        };
        // SAFETY: the host borrows both ids for this call.
        let (item, damage_type) = unsafe { (item.str(), damage_type.str()) };
        handler(world, item, attacker, target, damage_type, amount)
    })
}
