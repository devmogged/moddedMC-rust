//! `moddedmc:item_stacks` and `moddedmc:displays` v1: item entities, saved stacks and the item
//! displays mods draw for items that aren't entities.

use std::{
    collections::HashMap,
    io::Cursor,
    sync::{Arc, LazyLock, Mutex, PoisonError, atomic::Ordering},
};

use moddedmc_api::abi::{self, EntityRef, Sink, Str, WorldRef};
use pumpkin_core::{
    entity::{Entity, EntityBase, decoration::display::ItemDisplayEntity, item::ItemEntity},
    plugin::api::events::entity::item_spawn::ItemSpawnEvent,
};
use pumpkin_data::{Block, entity::EntityType, item_stack::ItemStack};
use pumpkin_nbt::{Nbt, deserializer::NbtReadHelperJava};
use pumpkin_util::math::vector3::Vector3;

use crate::content::{entity, shared, world};

/// Vanilla `Display.TELEPORT_DURATION` is clamped to this.
const MAX_TELEPORT_DURATION: i32 = 59;
/// Vanilla `ItemDisplayContext.ON_SHELF`, the highest id.
const MAX_DISPLAY_CONTEXT: u8 = 9;

static DISPLAYS: LazyLock<Mutex<HashMap<i32, Arc<ItemDisplayEntity>>>> =
    LazyLock::new(Mutex::default);

pub fn read_stack(stack: Str) -> Option<ItemStack> {
    if stack.len > abi::MAX_STACK_BYTES {
        return None;
    }
    // SAFETY: the caller borrows the bytes for this call.
    let bytes = unsafe { stack.bytes() };
    let mut reader = Cursor::new(bytes);
    let nbt = Nbt::read(&mut NbtReadHelperJava::new(&mut reader)).ok()?;
    if reader.position() != bytes.len() as u64 {
        return None;
    }
    ItemStack::read_item_stack(&nbt.root_tag)
}

fn finite(abi::Vec3([x, y, z]): abi::Vec3) -> Option<Vector3<f64>> {
    (x.is_finite() && y.is_finite() && z.is_finite()).then(|| Vector3::new(x, y, z))
}

fn item_entity<'a>(raw: EntityRef) -> Option<&'a ItemEntity> {
    // SAFETY: the entity is live for the callback.
    unsafe { entity(raw) }.and_then(EntityBase::get_item_entity)
}

pub unsafe extern "C" fn entity_stack(raw: EntityRef, out: Sink) -> bool {
    let Some(item) = item_entity(raw) else {
        return false;
    };
    let Some(saved) = item
        .get_item_stack()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .to_nbt()
    else {
        return false;
    };
    let bytes = Nbt::from(saved).write();
    // SAFETY: the bytes are live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

pub unsafe extern "C" fn set_entity_stack(raw: EntityRef, stack: Str) -> bool {
    let Some(item) = item_entity(raw) else {
        return false;
    };
    if stack.len == 0 {
        // Vanilla `discard()`.
        item.get_entity().remove();
        return true;
    }
    let Some(stack) = read_stack(stack) else {
        return false;
    };
    *item
        .get_item_stack()
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = stack;
    // Vanilla `setItem` resyncs DATA_ITEM.
    item.init_data_tracker();
    true
}

pub unsafe extern "C" fn spawn(
    raw: WorldRef,
    position: abi::Vec3,
    motion: abi::Vec3,
    stack: Str,
) -> i32 {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(position), Some(motion), Some(stack)) = (
        unsafe { world(raw) }.and_then(shared),
        finite(position),
        finite(motion),
        read_stack(stack),
    ) else {
        return 0;
    };
    let entity = Entity::new(world.clone(), position, &EntityType::ITEM);
    let id = entity.entity_id;
    // Keep World::drop_stack's plugin contract.
    let mut event = ItemSpawnEvent::new(id, position, stack.item.registry_key.to_string());
    if let Some(server) = world.server.upgrade() {
        server.plugin_manager.fire_blocking(&server, &mut event);
    }
    if event.cancelled {
        return 0;
    }
    let item =
        ItemEntity::new_with_velocity(entity, stack, motion, ItemEntity::DEFAULT_PICKUP_DELAY);
    if world.spawn_entity(Arc::new(item)) {
        id
    } else {
        0
    }
}

pub unsafe extern "C" fn info(stack: Str, out: *mut abi::StackInfo) -> bool {
    let (Some(stack), false) = (read_stack(stack), out.is_null()) else {
        return false;
    };
    let info = abi::StackInfo {
        count: u32::from(stack.item_count),
        max_count: u32::from(stack.get_max_stack_size()),
        block: Block::from_item_id(stack.item.id).is_some_and(|block| block != &Block::AIR),
    };
    // SAFETY: out is a live, aligned StackInfo owned by the caller.
    unsafe { out.write(info) };
    true
}

pub static ITEM_STACKS: abi::ItemStacksV1 = abi::ItemStacksV1 {
    size: size_of::<abi::ItemStacksV1>(),
    entity_stack,
    set_entity_stack,
    spawn,
    info,
};

/// Validated display fields, or `None` when any is out of range.
fn display(display: *const abi::ItemDisplay) -> Option<(Vector3<f64>, abi::ItemDisplay)> {
    // SAFETY: the caller borrows the display for this call.
    let display = *unsafe { display.as_ref() }?;
    let position = finite(abi::Vec3(display.position))?;
    let valid = display.rotation.iter().all(|value| value.is_finite())
        && display.scale.is_finite()
        && display.context <= MAX_DISPLAY_CONTEXT
        && (0..=MAX_TELEPORT_DURATION).contains(&display.interpolation);
    valid.then_some((position, display))
}

fn apply(entity: &ItemDisplayEntity, display: &abi::ItemDisplay) {
    let shown = &entity.display;
    shown.set_teleport_duration(display.interpolation);
    shown.set_left_rotation(display.rotation);
    shown.set_scale(Vector3::new(display.scale, display.scale, display.scale));
    entity.set_item_display_mode(display.context);
}

fn live(id: i32) -> Option<Arc<ItemDisplayEntity>> {
    let mut displays = DISPLAYS.lock().unwrap_or_else(PoisonError::into_inner);
    let entity = displays.get(&id)?.clone();
    if entity.display.entity.removed.load(Ordering::Relaxed) {
        displays.remove(&id);
        return None;
    }
    Some(entity)
}

unsafe extern "C" fn spawn_item(raw: WorldRef, stack: Str, shown: *const abi::ItemDisplay) -> i32 {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(stack), Some((position, shown))) = (
        unsafe { world(raw) }.and_then(shared),
        read_stack(stack),
        display(shown),
    ) else {
        return 0;
    };
    let entity = Entity::new(world.clone(), position, &EntityType::ITEM_DISPLAY);
    entity.set_has_no_gravity(true);
    let id = entity.entity_id;
    let display = ItemDisplayEntity::transient(entity);
    display.init_data_tracker();
    display.set_item(stack);
    apply(&display, &shown);
    world.spawn_entity_non_save(display.clone());
    DISPLAYS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, display);
    id
}

unsafe extern "C" fn update(raw: WorldRef, id: i32, shown: *const abi::ItemDisplay) -> bool {
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

unsafe extern "C" fn set_item(raw: WorldRef, id: i32, stack: Str) -> bool {
    // SAFETY: the world is borrowed for this call.
    let (Some(_), Some(entity), Some(stack)) = (unsafe { world(raw) }, live(id), read_stack(stack))
    else {
        return false;
    };
    entity.set_item(stack);
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

pub static DISPLAYS_TABLE: abi::DisplaysV1 = abi::DisplaysV1 {
    size: size_of::<abi::DisplaysV1>(),
    spawn_item,
    update,
    set_item,
    remove,
};
