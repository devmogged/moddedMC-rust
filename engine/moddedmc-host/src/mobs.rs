//! `moddedmc:mobs`: mod mob types carried by vanilla mobs. A spawned mob's custom data
//! (`data.moddedmc`) names its type and holds the mod's state, so both are saved with it.

use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::abi::{self, EntityRef, Reg, Sink, Str, Vec3, WorldRef, code};
use pumpkin_core::entity::{EntityBase, r#type::from_type};
use pumpkin_data::{
    attributes::Attributes, data_component_impl::combat::attribute_from_name, entity::EntityType,
};
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;

use crate::{
    MAX_STATE_BYTES,
    content::{entity, shared, world},
    native,
};

pub static MOBS: abi::MobsV1 = abi::MobsV1 {
    size: size_of::<abi::MobsV1>(),
    add_mob,
    spawn,
    kind,
    state,
    set_state,
};

const NAMESPACE: &str = "moddedmc";
const TYPE_KEY: &str = "mob";
const STATE_KEY: &str = "state";

pub struct Registration {
    id: &'static str,
    carrier: &'static EntityType,
    attributes: Vec<(&'static Attributes, f64)>,
}

/// Registered mob types by id. Ids are leaked once at registration so `kind` can return them.
static TYPES: Mutex<BTreeMap<&'static str, Registration>> = Mutex::new(BTreeMap::new());

fn types() -> std::sync::MutexGuard<'static, BTreeMap<&'static str, Registration>> {
    TYPES.lock().unwrap_or_else(PoisonError::into_inner)
}

fn vanilla(name: &str) -> Option<&'static EntityType> {
    EntityType::from_name(name.strip_prefix("minecraft:").unwrap_or(name))
}

unsafe extern "C" fn add_mob(reg: Reg, def: *const abi::MobDefinition) -> i32 {
    // SAFETY: the host owns staging and the definition is borrowed by the registration call.
    let (staging, Some(def)) = (unsafe { (native::staging(reg), def.as_ref()) }) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: the strings and attribute array belong to the live definition.
    let (id, carrier, attributes) = unsafe {
        (
            def.id.str(),
            def.carrier.str(),
            if def.attribute_count == 0 || def.attributes.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts(def.attributes, def.attribute_count)
            },
        )
    };
    // A living carrier other than a player: it has to hold attributes and run goals.
    let Some(carrier) =
        vanilla(carrier).filter(|t| !t.attributes.is_empty() && **t != EntityType::PLAYER)
    else {
        return code::INVALID_DEFINITION;
    };
    let Some(attributes) = attributes
        .iter()
        // SAFETY: the attribute ids belong to the live definition.
        .map(|a| Some((attribute_from_name(unsafe { a.id.str() })?, a.value)))
        .filter(|a| a.is_none_or(|(_, value)| value.is_finite()))
        .collect::<Option<Vec<_>>>()
    else {
        return code::INVALID_DEFINITION;
    };
    if !crate::valid_id(id) {
        return code::INVALID_DEFINITION;
    }
    if types().contains_key(id) || staging.mobs.iter().any(|m| m.id == id) {
        return code::DUPLICATE_ID;
    }
    staging.mobs.push(Registration {
        id: String::leak(id.to_owned()),
        carrier,
        attributes,
    });
    code::OK
}

/// Commit a mod's staged mob types after its registration succeeded.
pub fn register(mobs: Vec<Registration>) {
    let mut types = types();
    for mob in mobs {
        tracing::info!(
            "moddedmc: registered mob {} (carrier minecraft:{})",
            mob.id,
            mob.carrier.resource_name
        );
        types.insert(mob.id, mob);
    }
}

unsafe extern "C" fn spawn(raw: WorldRef, kind: Str, position: Vec3) -> i32 {
    // SAFETY: the world and the kind are borrowed for this call.
    let (Some(world), kind) = (unsafe { world(raw) }.and_then(shared), unsafe {
        kind.str()
    }) else {
        return -1;
    };
    let abi::Vec3([x, y, z]) = position;
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return -1;
    }
    let types = types();
    let (carrier, registered) = match types.get(kind) {
        Some(mob) => (mob.carrier, Some(mob)),
        None => match vanilla(kind) {
            Some(carrier) => (carrier, None),
            None => return -1,
        },
    };
    let mob = from_type(carrier, Vector3::new(x, y, z), &world, Uuid::new_v4());
    if mob.get_mob().is_none() {
        return -1;
    }
    if let Some(registered) = registered {
        let entity = mob.get_entity();
        entity.set_custom_data(NAMESPACE, TYPE_KEY, NbtTag::String(registered.id.into()));
        if let Some(living) = mob.get_living_entity() {
            for (attribute, value) in &registered.attributes {
                living.set_attribute_base(attribute, *value);
            }
            if let Some((_, health)) = registered
                .attributes
                .iter()
                .find(|(a, _)| **a == Attributes::MAX_HEALTH)
            {
                living.set_health(*health as f32);
            }
        }
    }
    drop(types);
    let id = mob.get_entity().entity_id;
    if world.spawn_entity(mob) { id } else { -1 }
}

/// The registered type a mob's custom data names, if that type is still registered.
fn registered_kind(base: &dyn EntityBase) -> Option<&'static str> {
    let NbtTag::String(name) = base.get_entity().get_custom_data(NAMESPACE, TYPE_KEY)? else {
        return None;
    };
    types().get_key_value(&*name).map(|(id, _)| *id)
}

unsafe extern "C" fn kind(raw: EntityRef) -> Str {
    // SAFETY: the entity is live for the callback.
    let id = unsafe { entity(raw) }
        .and_then(registered_kind)
        .unwrap_or("");
    Str::new(id.as_bytes())
}

unsafe extern "C" fn state(raw: EntityRef, out: Sink) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }).filter(|b| registered_kind(*b).is_some()) else {
        return false;
    };
    let bytes: Vec<u8> = match base.get_entity().get_custom_data(NAMESPACE, STATE_KEY) {
        Some(NbtTag::ByteArray(bytes)) => bytes.iter().map(|&b| b as u8).collect(),
        _ => Vec::new(),
    };
    // SAFETY: the sink is valid for this call.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn set_state(raw: EntityRef, state: Str) -> bool {
    // SAFETY: the entity and bytes are borrowed for this call.
    let (Some(base), bytes) = (unsafe { entity(raw) }, unsafe { state.bytes() }) else {
        return false;
    };
    if bytes.len() > MAX_STATE_BYTES || registered_kind(base).is_none() {
        return false;
    }
    let bytes: Box<[i8]> = bytes.iter().map(|&b| b as i8).collect();
    base.get_entity()
        .set_custom_data(NAMESPACE, STATE_KEY, NbtTag::ByteArray(bytes));
    true
}
