//! `moddedmc:block_displays` v1: transient block displays for blocks that aren't in the world.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, PoisonError, atomic::Ordering},
};

use moddedmc_api::abi::{self, WorldRef};
use pumpkin_core::entity::{Entity, EntityBase, decoration::display::BlockDisplayEntity};
use pumpkin_data::{BlockStateId, entity::EntityType};
use pumpkin_util::math::vector3::Vector3;

use crate::content::{shared, world};

/// Vanilla `Display.TELEPORT_DURATION` is clamped to this.
const MAX_TELEPORT_DURATION: i32 = 59;
/// Vanilla `Brightness.pack`: block light in bits 4 to 7, sky light in bits 20 to 23.
const BRIGHTNESS_BITS: i32 = 0xF << 4 | 0xF << 20;

static DISPLAYS: LazyLock<Mutex<HashMap<i32, Arc<BlockDisplayEntity>>>> =
    LazyLock::new(Mutex::default);

fn finite<const N: usize, T: Into<f64> + Copy>(values: [T; N]) -> bool {
    values.iter().all(|value| (*value).into().is_finite())
}

/// -1, or a vanilla `Brightness.pack` value.
const fn valid_brightness(packed: i32) -> bool {
    packed == -1 || packed & !BRIGHTNESS_BITS == 0
}

/// Validated display fields, or `None` when any is out of range.
fn display(display: *const abi::BlockDisplay) -> Option<(Vector3<f64>, abi::BlockDisplay)> {
    // SAFETY: the caller borrows the display for this call.
    let display = *unsafe { display.as_ref() }?;
    let valid = finite(display.position)
        && finite(display.translation)
        && finite(display.rotation)
        && finite(display.scale)
        && display.transform_interpolation >= 0
        && (0..=MAX_TELEPORT_DURATION).contains(&display.position_interpolation)
        && valid_brightness(display.brightness);
    let [x, y, z] = display.position;
    valid.then(|| (Vector3::new(x, y, z), display))
}

fn state(state: u32) -> Option<i32> {
    let id = u16::try_from(state).ok()?;
    BlockStateId::from_runtime_id(id)?;
    Some(i32::from(id))
}

/// Vanilla starts a new transformation blend when `interpolation_start_delta_ticks` is set
/// alongside the new transformation.
fn apply(entity: &BlockDisplayEntity, display: &abi::BlockDisplay) {
    let shown = &entity.display;
    let [x, y, z] = display.translation;
    let [sx, sy, sz] = display.scale;
    shown.set_teleport_duration(display.position_interpolation);
    shown.set_interpolation_duration(display.transform_interpolation);
    shown.set_translation(Vector3::new(x, y, z));
    shown.set_left_rotation(display.rotation);
    shown.set_scale(Vector3::new(sx, sy, sz));
    shown.set_brightness(display.brightness);
    shown.set_interpolation_start_delta_ticks(0);
}

fn live(id: i32) -> Option<Arc<BlockDisplayEntity>> {
    let mut displays = DISPLAYS.lock().unwrap_or_else(PoisonError::into_inner);
    let entity = displays.get(&id)?.clone();
    if entity.display.entity.removed.load(Ordering::Relaxed) {
        displays.remove(&id);
        return None;
    }
    Some(entity)
}

unsafe extern "C" fn spawn(raw: WorldRef, block: u32, shown: *const abi::BlockDisplay) -> i32 {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(block), Some((position, shown))) = (
        unsafe { world(raw) }.and_then(shared),
        state(block),
        display(shown),
    ) else {
        return 0;
    };
    let entity = Entity::new(world.clone(), position, &EntityType::BLOCK_DISPLAY);
    entity.set_has_no_gravity(true);
    let id = entity.entity_id;
    let display = BlockDisplayEntity::transient(entity);
    display.init_data_tracker();
    display.set_block_state(block);
    apply(&display, &shown);
    world.spawn_entity_non_save(display.clone());
    DISPLAYS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, display);
    id
}

unsafe extern "C" fn update(raw: WorldRef, id: i32, shown: *const abi::BlockDisplay) -> bool {
    // SAFETY: the world is borrowed for this call.
    let (Some(_), Some(entity), Some((position, shown))) =
        (unsafe { world(raw) }, live(id), display(shown))
    else {
        return false;
    };
    apply(&entity, &shown);
    entity.display.entity.set_pos(position);
    true
}

unsafe extern "C" fn set_state(raw: WorldRef, id: i32, block: u32) -> bool {
    // SAFETY: the world is borrowed for this call.
    let (Some(_), Some(entity), Some(block)) = (unsafe { world(raw) }, live(id), state(block))
    else {
        return false;
    };
    entity.set_block_state(block);
    true
}

unsafe extern "C" fn remove(raw: WorldRef, id: i32) {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return;
    };
    let entity = DISPLAYS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
    if let Some(entity) = entity {
        world.remove_entity(entity.as_ref());
    }
}

pub static BLOCK_DISPLAYS: abi::BlockDisplaysV1 = abi::BlockDisplaysV1 {
    size: size_of::<abi::BlockDisplaysV1>(),
    spawn,
    update,
    set_state,
    remove,
};
