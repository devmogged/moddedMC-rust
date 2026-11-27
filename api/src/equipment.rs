//! Worn-armor and held-weapon hooks: change or cancel damage a player takes or deals.
use std::sync::OnceLock;

use crate::{
    InterfacePrefix, InterfaceUnavailable, RegistrationError, Registry, abi, content::World,
    entities::Entity, guard, s,
};

#[derive(Clone, Copy)]
pub struct Equipment(InterfacePrefix<abi::EquipmentV1>);

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
pub type SuccessfulWeaponHit = fn(World<'_>, &str, Entity<'_>, Entity<'_>, f32);
static SUCCESSFUL_WEAPON: OnceLock<SuccessfulWeaponHit> = OnceLock::new();

pub fn require() -> Result<Equipment, InterfaceUnavailable> {
    // SAFETY: EquipmentV1 is the published table type; only the original prefix is required.
    unsafe {
        InterfacePrefix::require(
            abi::EQUIPMENT,
            1,
            std::mem::offset_of!(abi::EquipmentV1, worn_armor),
        )
    }
    .map(Equipment)
}

/// One handler per mod for each hook; it receives the item id.
fn same<F: Copy>(slot: &OnceLock<F>, handler: F, eq: fn(F, F) -> bool) -> bool {
    eq(*slot.get_or_init(|| handler), handler)
}

impl Equipment {
    pub fn require_successful_weapon_hit(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(std::mem::offset_of!(
                abi::EquipmentV1,
                on_successful_weapon_hit
            ))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:equipment.on_successful_weapon_hit",
                version: 1,
            })
    }

    pub fn on_successful_weapon_hit(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        handler: SuccessfulWeaponHit,
    ) -> Result<(), RegistrationError> {
        if !same(&SUCCESSFUL_WEAPON, handler, std::ptr::fn_addr_eq) {
            return Err(RegistrationError::InvalidDefinition);
        }
        let Some(api) = self.0.upto(std::mem::offset_of!(
            abi::EquipmentV1,
            on_successful_weapon_hit
        )) else {
            return Err(RegistrationError::InvalidDefinition);
        };
        // SAFETY: registration and item are borrowed; the trampoline is static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*api).on_successful_weapon_hit).read())(
                reg.reg,
                s(item),
                successful_weapon,
            )
        })
    }

    pub fn on_worn_hurt(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        handler: WornHurt,
    ) -> Result<(), RegistrationError> {
        if !same(&WORN, handler, std::ptr::fn_addr_eq) {
            return Err(RegistrationError::InvalidDefinition);
        }
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::EquipmentV1, on_worn_hurt))
        else {
            return Err(RegistrationError::InvalidDefinition);
        };
        // SAFETY: the field is present, registration and item id are borrowed, trampoline is static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*api).on_worn_hurt).read())(reg.reg, s(item), worn)
        })
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
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::EquipmentV1, on_weapon_hit))
        else {
            return Err(RegistrationError::InvalidDefinition);
        };
        // SAFETY: the field is present, registration and item id are borrowed, trampoline is static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*api).on_weapon_hit).read())(reg.reg, s(item), weapon)
        })
    }

    /// Read the head, chest, legs and feet item IDs (minecraft:air for empty slots).
    /// None for non-players or hosts without the appended call.
    pub fn worn_armor(self, player: Entity<'_>) -> Option<[String; 4]> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::EquipmentV1, worn_armor))?;
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the field is present, the entity is live and the sink collects synchronously.
        if !unsafe { ((&raw const (*api).worn_armor).read())(player.raw(), sink) } {
            return None;
        }
        decode_armor(&data)
    }
}

fn decode_armor(data: &[u8]) -> Option<[String; 4]> {
    let mut ids = std::str::from_utf8(data).ok()?.split('\n');
    let armor = [ids.next()?, ids.next()?, ids.next()?, ids.next()?];
    if ids.next().is_some() || armor.contains(&"") {
        return None;
    }
    Some(armor.map(str::to_owned))
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

unsafe extern "C" fn successful_weapon(
    world: abi::WorldRef,
    item: abi::Str,
    attacker: abi::EntityRef,
    target: abi::EntityRef,
    amount: f32,
) {
    guard("successful weapon hit", (), || {
        let (Some(handler), Some(world), Some(attacker), Some(target)) = (
            SUCCESSFUL_WEAPON.get(),
            World::borrowed(world),
            Entity::borrowed(attacker),
            Entity::borrowed(target),
        ) else {
            return;
        };
        // SAFETY: the host borrows the item id for the call.
        handler(world, unsafe { item.str() }, attacker, target, amount);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armor_order_and_invalid_payloads() {
        assert_eq!(
            decode_armor(b"minecraft:air\nmod:chest\nminecraft:iron_leggings\nmod:boots"),
            Some(
                [
                    "minecraft:air",
                    "mod:chest",
                    "minecraft:iron_leggings",
                    "mod:boots"
                ]
                .map(str::to_owned)
            )
        );
        assert_eq!(decode_armor(b"a\nb\nc"), None);
        assert_eq!(decode_armor(b"a\nb\nc\nd\ne"), None);
        assert_eq!(decode_armor(b"a\n\nc\nd"), None);
        assert_eq!(decode_armor(&[255]), None);
    }

    #[test]
    fn original_prefix_has_hooks_but_no_armor_read() {
        let prefix = InterfacePrefix::<abi::EquipmentV1> {
            ptr: std::ptr::null(),
            size: std::mem::offset_of!(abi::EquipmentV1, worn_armor),
        };
        assert!(
            prefix
                .upto(std::mem::offset_of!(abi::EquipmentV1, on_worn_hurt))
                .is_some()
        );
        assert!(
            prefix
                .upto(std::mem::offset_of!(abi::EquipmentV1, on_weapon_hit))
                .is_some()
        );
        assert!(
            prefix
                .upto(std::mem::offset_of!(abi::EquipmentV1, worn_armor))
                .is_none()
        );
    }
}
