//! The entity functions appended to `moddedmc:entities` v1: ids, lookup, movement, effects,
//! step height and box queries.

use std::{ffi::c_void, sync::atomic::Ordering};

use moddedmc_api::abi::{self, Str, WorldRef};
use pumpkin_core::entity::{EntityBase, living::LivingEntity, player::Player};
use pumpkin_data::{attributes::Attributes, effect::StatusEffect, potion::Effect};
use pumpkin_util::math::{boundingbox::BoundingBox, vector3::Vector3};

use crate::content::{entity, world};

fn finite(abi::Vec3([x, y, z]): abi::Vec3) -> Option<Vector3<f64>> {
    (x.is_finite() && y.is_finite() && z.is_finite()).then(|| Vector3::new(x, y, z))
}

pub unsafe extern "C" fn id(raw: abi::EntityRef) -> i32 {
    // SAFETY: the entity is live for the callback.
    unsafe { entity(raw) }.map_or(0, |base| base.get_entity().entity_id)
}

pub unsafe extern "C" fn with_entity(
    raw: WorldRef,
    id: i32,
    ctx: *mut c_void,
    visit: unsafe extern "C" fn(ctx: *mut c_void, entity: abi::EntityRef),
) -> bool {
    // SAFETY: the world is borrowed for this call.
    let Some(found) = (unsafe { world(raw) }).and_then(|world| world.get_entity_by_id(id)) else {
        return false;
    };
    let base: &dyn EntityBase = found.as_ref();
    // SAFETY: `base` outlives the call and the mod only uses it during `visit`.
    unsafe { visit(ctx, (&raw const base).cast_mut().cast()) };
    true
}

pub unsafe extern "C" fn flags(raw: abi::EntityRef) -> u32 {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return 0;
    };
    let entity = base.get_entity();
    let alive = base
        .get_living_entity()
        .map_or_else(|| entity.is_alive(), LivingEntity::is_alive);
    let mut flags = 0;
    if alive {
        flags |= abi::ENTITY_ALIVE;
    }
    if entity.sneaking.load(Ordering::Relaxed) {
        flags |= abi::ENTITY_SNEAKING;
    }
    if base.get_player().is_some_and(Player::is_flying) {
        flags |= abi::ENTITY_FLYING;
    }
    flags
}

pub unsafe extern "C" fn move_by(raw: abi::EntityRef, delta: abi::Vec3) {
    // SAFETY: the entity is live for the callback.
    if let (Some(base), Some(delta)) = (unsafe { entity(raw) }, finite(delta)) {
        base.get_entity().move_entity(base, delta);
    }
}

pub unsafe extern "C" fn set_position(raw: abi::EntityRef, position: abi::Vec3) {
    // SAFETY: the entity is live for the callback.
    if let (Some(base), Some(position)) = (unsafe { entity(raw) }, finite(position)) {
        base.get_entity().set_pos(position);
    }
}

pub unsafe extern "C" fn body(raw: abi::EntityRef, out: *mut abi::EntityBody) -> bool {
    // SAFETY: the entity is live for the callback.
    let (Some(base), false) = (unsafe { entity(raw) }, out.is_null()) else {
        return false;
    };
    let bounds = base.get_entity().bounding_box.load();
    let input = base
        .get_living_entity()
        .map_or_else(Vector3::default, |living| living.movement_input.load());
    let body = abi::EntityBody {
        min: [bounds.min.x, bounds.min.y, bounds.min.z],
        max: [bounds.max.x, bounds.max.y, bounds.max.z],
        input: [input.x, input.y, input.z],
    };
    // SAFETY: out is a live, aligned EntityBody owned by the caller.
    unsafe { out.write(body) };
    true
}

pub unsafe extern "C" fn reset_fall_distance(raw: abi::EntityRef) {
    // SAFETY: the entity is live for the callback.
    if let Some(living) = unsafe { entity(raw) }.and_then(EntityBase::get_living_entity) {
        living.fall_distance.store(0.0);
    }
}

pub unsafe extern "C" fn add_effect(
    raw: abi::EntityRef,
    effect: Str,
    duration: i32,
    amplifier: u8,
    ambient: bool,
    visible: bool,
) -> bool {
    // SAFETY: the entity is live and the id is borrowed for the call.
    let (Some(base), name) = (unsafe { entity(raw) }, unsafe { effect.str() }) else {
        return false;
    };
    let Some(effect_type) =
        StatusEffect::from_name(name.strip_prefix("minecraft:").unwrap_or(name))
    else {
        return false;
    };
    let effect = Effect {
        effect_type,
        duration,
        amplifier,
        ambient,
        show_particles: visible,
        show_icon: visible,
        blend: false,
    };
    if let Some(player) = base.get_player() {
        player.add_effect(effect);
    } else if let Some(living) = base.get_living_entity() {
        living.add_effect(effect);
    } else {
        return false;
    }
    true
}

pub unsafe extern "C" fn step_height(raw: abi::EntityRef) -> f64 {
    // SAFETY: the entity is live for the callback.
    unsafe { entity(raw) }
        .and_then(EntityBase::get_living_entity)
        .map_or(f64::NAN, |living| {
            living.get_attribute_base(&Attributes::STEP_HEIGHT)
        })
}

pub unsafe extern "C" fn set_step_height(raw: abi::EntityRef, value: f64) {
    // SAFETY: the entity is live for the callback.
    let living = unsafe { entity(raw) }.and_then(EntityBase::get_living_entity);
    if let (Some(living), true) = (living, value.is_finite()) {
        living.set_attribute_base(&Attributes::STEP_HEIGHT, value);
    }
}

pub unsafe extern "C" fn entities_in(
    raw: WorldRef,
    min: abi::Vec3,
    max: abi::Vec3,
    except: i32,
    out: *mut i32,
    capacity: usize,
) -> usize {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(min), Some(max)) = (unsafe { world(raw) }, finite(min), finite(max))
    else {
        return 0;
    };
    let mut count = 0;
    for found in world.get_all_at_box(&BoundingBox::new(min, max)) {
        let id = found.get_entity().entity_id;
        if id == except {
            continue;
        }
        if count < capacity && !out.is_null() {
            // SAFETY: count is below the capacity of the caller's array.
            unsafe { out.add(count).write(id) };
        }
        count += 1;
    }
    count
}
