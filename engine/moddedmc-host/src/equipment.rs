//! `moddedmc:equipment`: hooks for content items a player wears or holds.
use std::{
    collections::HashMap,
    ptr,
    sync::{Arc, Mutex},
};

use moddedmc_api::abi::{self, Reg, Str, code};
use pumpkin_core::{
    entity::{EntityBase, player::Player},
    plugin::{
        BoxFuture, Cancellable, EventHandler, api::events::entity::entity_damage::EntityDamageEvent,
    },
    server::Server,
};
use pumpkin_data::data_component_impl::EquipmentSlot;

use crate::{lock, native};

pub static EQUIPMENT: abi::EquipmentV1 = abi::EquipmentV1 {
    size: size_of::<abi::EquipmentV1>(),
    on_worn_hurt,
    on_weapon_hit,
    worn_armor,
};

#[derive(Clone, Copy)]
pub enum Hook {
    Worn(abi::WornHurtFn),
    Weapon(abi::WeaponHitFn),
}

#[derive(Default)]
struct Hooks {
    worn: HashMap<String, abi::WornHurtFn>,
    weapon: HashMap<String, abi::WeaponHitFn>,
}

static HOOKS: Mutex<Option<Hooks>> = Mutex::new(None);

/// Stage `hook` for one of the registering mod's own content items.
unsafe fn stage(reg: Reg, item: Str, hook: Hook) -> i32 {
    // SAFETY: the host owns staging and the item id is borrowed for this call.
    let (staging, item) = unsafe { (native::staging(reg), item.str()) };
    if !staging.content_items.iter().any(|entry| entry.id == item) {
        return code::INVALID_DEFINITION;
    }
    let duplicate = staging.equipment.iter().any(|(id, staged)| {
        id == item
            && matches!(
                (staged, hook),
                (Hook::Worn(_), Hook::Worn(_)) | (Hook::Weapon(_), Hook::Weapon(_))
            )
    });
    if duplicate {
        return code::DUPLICATE_ID;
    }
    staging.equipment.push((item.to_owned(), hook));
    code::OK
}

unsafe extern "C" fn on_worn_hurt(reg: Reg, item: Str, callback: abi::WornHurtFn) -> i32 {
    // SAFETY: forwarded from the caller.
    unsafe { stage(reg, item, Hook::Worn(callback)) }
}

unsafe extern "C" fn on_weapon_hit(reg: Reg, item: Str, callback: abi::WeaponHitFn) -> i32 {
    // SAFETY: forwarded from the caller.
    unsafe { stage(reg, item, Hook::Weapon(callback)) }
}

/// Commit a mod's staged hooks after its registration succeeded.
pub fn register(staged: Vec<(String, Hook)>) {
    if staged.is_empty() {
        return;
    }
    let mut hooks = lock(&HOOKS);
    let hooks = hooks.get_or_insert_default();
    for (item, hook) in staged {
        match hook {
            Hook::Worn(callback) => hooks.worn.insert(item, callback).is_some(),
            Hook::Weapon(callback) => hooks.weapon.insert(item, callback).is_some(),
        };
    }
}

/// The damage `attacker` deals with a content weapon that registered a hit hook.
pub fn weapon_hit(item: &str, attacker: &Player, target: &dyn EntityBase, damage: f32) -> f32 {
    let Some(callback) = lock(&HOOKS)
        .as_ref()
        .and_then(|hooks| hooks.weapon.get(item).copied())
    else {
        return damage;
    };
    let world = attacker.living_entity.entity.world.load_full();
    let attacker: &dyn EntityBase = attacker;
    let damage_type = "minecraft:player_attack";
    // SAFETY: the world, both entities and the ids outlive this synchronous callback.
    let adjusted = unsafe {
        callback(
            ptr::from_ref(world.as_ref()).cast(),
            Str::new(item.as_bytes()),
            ptr::from_ref(&attacker).cast(),
            ptr::from_ref(&target).cast(),
            Str::new(damage_type.as_bytes()),
            damage,
        )
    };
    if adjusted.is_finite() && adjusted >= 0.0 {
        adjusted
    } else {
        damage
    }
}

const ARMOR: [EquipmentSlot; 4] = [
    EquipmentSlot::HEAD,
    EquipmentSlot::CHEST,
    EquipmentSlot::LEGS,
    EquipmentSlot::FEET,
];

fn armor_ids(player: &Player) -> [String; 4] {
    let equipment = lock(&player.inventory().entity_equipment);
    ARMOR.map(|slot| {
        let stack = equipment.get(&slot);
        let key = if stack.is_empty() {
            "air"
        } else {
            stack.item.registry_key
        };
        if key.contains(':') {
            key.to_owned()
        } else {
            format!("minecraft:{key}")
        }
    })
}

unsafe extern "C" fn worn_armor(raw: abi::EntityRef, out: abi::Sink) -> bool {
    // SAFETY: the entity is borrowed for the active callback.
    let Some(player) = (unsafe { crate::content::entity(raw) }).and_then(EntityBase::get_player)
    else {
        return false;
    };
    let ids = armor_ids(player).join("\n");
    // SAFETY: the IDs live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(ids.as_bytes())) };
    true
}

/// Lets each distinct worn content item with a hurt hook change or cancel a player's damage.
pub struct WornHurtHandler;

impl EventHandler<EntityDamageEvent> for WornHurtHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut EntityDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() {
                return;
            }
            let (player, armor, worn) = {
                let hooks = lock(&HOOKS);
                let Some(hooks) = hooks.as_ref().filter(|hooks| !hooks.worn.is_empty()) else {
                    return;
                };
                let Some(player) = server
                    .worlds
                    .load()
                    .iter()
                    .find_map(|world| world.get_player_by_id(event.entity_id))
                else {
                    return;
                };
                let armor = armor_ids(&player);
                let mut worn: Vec<(String, abi::WornHurtFn)> = Vec::new();
                for key in &armor {
                    if let Some(&callback) = hooks.worn.get(key)
                        && !worn.iter().any(|(id, _)| id == key)
                    {
                        worn.push((key.clone(), callback));
                    }
                }
                (player, armor, worn)
            };
            if worn.is_empty() {
                return;
            }
            let ids = [0, 1, 2, 3].map(|slot| Str::new(armor[slot].as_bytes()));
            // `registry_key` is the death message id (`inFire`); this is `minecraft:in_fire`.
            let name = crate::mob_ai::damage_type_name(server, &event.damage_type);
            let world = player.living_entity.entity.world.load_full();
            let fall = player.living_entity.fall_distance.load();
            let base: &dyn EntityBase = player.as_ref();
            for (item, callback) in worn {
                // SAFETY: the world, player and ids outlive this synchronous callback.
                let amount = crate::mob_ai::with_damage_source(event, || unsafe {
                    callback(
                        ptr::from_ref(world.as_ref()).cast(),
                        Str::new(item.as_bytes()),
                        ptr::from_ref(&base).cast(),
                        &raw const ids,
                        Str::new(name.as_bytes()),
                        event.damage,
                        fall,
                    )
                });
                if amount < 0.0 {
                    event.set_cancelled(true);
                    return;
                }
                if amount.is_finite() {
                    event.damage = amount;
                }
            }
        })
    }
}
