//! The entity functions appended to `moddedmc:entities` v1: ids, lookup, movement, effects,
//! step height, box queries and mod data saved with the entity.

use std::{
    ffi::c_void,
    io::Cursor,
    sync::{Arc, PoisonError, atomic::Ordering},
};

use moddedmc_api::abi::{self, Str, WorldRef};
use pumpkin_core::entity::{
    Entity, EntityBase, living::LivingEntity, player::Player, r#type::from_type,
};
use pumpkin_data::{
    attributes::Attributes, effect::StatusEffect, entity::EntityType, potion::Effect,
};
use pumpkin_nbt::{Nbt, deserializer::NbtReadHelperJava};
use pumpkin_protocol::java::server::play::SPlayerInput;
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};
use uuid::Uuid;

use crate::content::{entity, shared, world};

/// Vanilla `HangingEntity` subclasses.
const HANGING: [&EntityType; 3] = [
    &EntityType::PAINTING,
    &EntityType::ITEM_FRAME,
    &EntityType::GLOW_ITEM_FRAME,
];
/// Entities whose vanilla `getPistonPushReaction()` is always `IGNORE_ENTITY`.
const PUSH_IGNORED: [&EntityType; 7] = [
    &EntityType::AREA_EFFECT_CLOUD,
    &EntityType::BLOCK_DISPLAY,
    &EntityType::ITEM_DISPLAY,
    &EntityType::TEXT_DISPLAY,
    &EntityType::INTERACTION,
    &EntityType::MARKER,
    &EntityType::OMINOUS_ITEM_SPAWNER,
];
/// Hops `has_indirect_passenger` follows before giving up on a malformed ride chain.
const MAX_RIDE_DEPTH: usize = 64;

fn finite(abi::Vec3([x, y, z]): abi::Vec3) -> Option<Vector3<f64>> {
    (x.is_finite() && y.is_finite() && z.is_finite()).then(|| Vector3::new(x, y, z))
}

pub unsafe extern "C" fn with_player_context(
    raw: abi::PlayerRef,
    context: *mut c_void,
    visit: unsafe extern "C" fn(*mut c_void, WorldRef, abi::EntityRef),
) -> bool {
    // SAFETY: the player handle was borrowed by the active mod callback.
    let Some(player) = (unsafe { crate::native::player(raw) }) else {
        return false;
    };
    let world = player.world();
    let player: &dyn EntityBase = player;
    // SAFETY: the world Arc, entity reference and caller's context outlive this synchronous visit.
    unsafe {
        visit(
            context,
            Arc::as_ptr(&world).cast(),
            (&raw const player).cast(),
        );
    };
    true
}

pub unsafe extern "C" fn as_player(raw: abi::EntityRef) -> abi::PlayerRef {
    // SAFETY: the source entity and the resulting player are borrowed for the callback.
    let player = unsafe { entity(raw) }.and_then(EntityBase::get_player);
    abi::PlayerRef(player.map_or(std::ptr::null(), |player| std::ptr::from_ref(player).cast()))
}

pub unsafe extern "C" fn has_scoreboard_tag(raw: abi::EntityRef, tag: Str) -> bool {
    // SAFETY: the entity and the tag are borrowed for this call.
    let (Some(base), tag) = (unsafe { entity(raw) }, unsafe { tag.str() }) else {
        return false;
    };
    base.get_entity()
        .scoreboard_tags
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(tag)
}

pub unsafe extern "C" fn id(raw: abi::EntityRef) -> i32 {
    // SAFETY: the entity is live for the callback.
    unsafe { entity(raw) }.map_or(0, |base| base.get_entity().entity_id)
}

pub unsafe extern "C" fn age(raw: abi::EntityRef) -> i32 {
    // SAFETY: the entity is borrowed for the callback.
    unsafe { entity(raw) }.map_or(-1, |base| {
        base.get_entity().tick_count.load(Ordering::Relaxed)
    })
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

pub unsafe extern "C" fn hurt(raw: abi::EntityRef, damage_type: Str, amount: f32) -> bool {
    if !amount.is_finite() || amount < 0.0 || damage_type.len == 0 || damage_type.len > 256 {
        return false;
    }
    // SAFETY: the entity handle and damage key are borrowed for this call.
    let (Some(entity), Ok(key)) = (
        unsafe { entity(raw) },
        std::str::from_utf8(unsafe { damage_type.bytes() }),
    ) else {
        return false;
    };
    let Some(server) = entity.get_entity().world.load().server.upgrade() else {
        return false;
    };
    server
        .datapack_manager
        .get_damage_type(key)
        .or_else(|| {
            // Like the damage command, fall back to the generated vanilla registry.
            let name = key.strip_prefix("minecraft:").unwrap_or(key);
            pumpkin_data::damage::DamageType::from_name(name)
        })
        .is_some_and(|damage| entity.damage_with_context(entity, amount, damage, None, None, None))
}

pub unsafe extern "C" fn hurt_by(
    raw: abi::EntityRef,
    attacker: i32,
    damage_type: Str,
    amount: f32,
) -> bool {
    if !amount.is_finite() || amount < 0.0 || damage_type.len == 0 || damage_type.len > 256 {
        return false;
    }
    // SAFETY: the entity and damage type are borrowed for this call.
    let (Some(target), Ok(key)) = (
        unsafe { entity(raw) },
        std::str::from_utf8(unsafe { damage_type.bytes() }),
    ) else {
        return false;
    };
    let world = target.get_entity().world.load();
    let (Some(attacker), Some(server)) = (world.get_entity_by_id(attacker), world.server.upgrade())
    else {
        return false;
    };
    server
        .datapack_manager
        .get_damage_type(key)
        .or_else(|| {
            let name = key.strip_prefix("minecraft:").unwrap_or(key);
            pumpkin_data::damage::DamageType::from_name(name)
        })
        .is_some_and(|damage| {
            target.damage_with_context(
                target,
                amount,
                damage,
                None,
                Some(attacker.as_ref()),
                Some(attacker.as_ref()),
            )
        })
}

pub unsafe extern "C" fn remove_effect(raw: abi::EntityRef, effect: Str) -> bool {
    // SAFETY: the entity and effect are borrowed for this call.
    let (Some(base), name) = (unsafe { entity(raw) }, unsafe { effect.str() }) else {
        return false;
    };
    let Some(effect) = StatusEffect::from_name(name.strip_prefix("minecraft:").unwrap_or(name))
    else {
        return false;
    };
    base.get_player().map_or_else(
        || {
            base.get_living_entity()
                .is_some_and(|living| living.remove_effect(effect))
        },
        |player| player.remove_effect(effect),
    )
}

pub unsafe extern "C" fn attribute_value(raw: abi::EntityRef, attribute: Str) -> f64 {
    use pumpkin_data::data_component_impl::combat::attribute_from_name;
    // SAFETY: the entity and attribute are borrowed for this call.
    let (Some(living), Some(attribute)) = (
        unsafe { entity(raw) }.and_then(EntityBase::get_living_entity),
        attribute_from_name(unsafe { attribute.str() }),
    ) else {
        return f64::NAN;
    };
    living.get_attribute_value(attribute)
}

pub unsafe extern "C" fn ticks_frozen(raw: abi::EntityRef) -> i32 {
    // SAFETY: the entity is borrowed for this call.
    unsafe { entity(raw) }.map_or(-1, |base| base.get_entity().get_frozen_ticks())
}

pub unsafe extern "C" fn set_ticks_frozen(raw: abi::EntityRef, ticks: i32) -> bool {
    // SAFETY: the entity is borrowed for this call.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    if ticks < 0 {
        return false;
    }
    base.get_entity().set_frozen_ticks(ticks);
    true
}

pub unsafe extern "C" fn spawn_projectile(
    raw: WorldRef,
    kind: Str,
    owner: i32,
    target: i32,
    position: abi::Vec3,
    velocity: abi::Vec3,
) -> i32 {
    use pumpkin_core::entity::{
        mob::shulker::Axis, projectile::shulker_bullet::ShulkerBulletEntity,
    };
    // SAFETY: the world and kind are borrowed for this call.
    let (Some(world), Some(position), Some(velocity)) = (
        unsafe { world(raw) }.and_then(shared),
        finite(position),
        finite(velocity),
    ) else {
        return -1;
    };
    // SAFETY: the projectile kind is borrowed for this call.
    let name = unsafe { kind.str() };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    if !matches!(
        name,
        "arrow"
            | "spectral_arrow"
            | "trident"
            | "fireball"
            | "small_fireball"
            | "dragon_fireball"
            | "wither_skull"
            | "shulker_bullet"
            | "snowball"
            | "egg"
            | "ender_pearl"
            | "experience_bottle"
            | "llama_spit"
            | "wind_charge"
            | "breeze_wind_charge"
    ) {
        return -1;
    }
    let Some(kind) = EntityType::from_name(name) else {
        return -1;
    };
    let Some(owner) = world.get_entity_by_id(owner) else {
        return -1;
    };
    if world
        .get_block_state_id_if_loaded(&BlockPos::floored_v(position))
        .is_none()
    {
        return -1;
    }
    let owner = owner.get_entity();
    let projectile: Arc<dyn EntityBase> = if kind == &EntityType::SHULKER_BULLET {
        let Some(target) = world.get_entity_by_id(target) else {
            return -1;
        };
        let target = target.get_entity();
        let bullet = ShulkerBulletEntity::new(owner, target.entity_id, target.pos.load(), Axis::Y);
        bullet.entity.set_pos(position);
        Arc::new(bullet)
    } else {
        let projectile = from_type(kind, position, &world, Uuid::new_v4());
        let Some(reference) = projectile.get_projectile_owner() else {
            return -1;
        };
        reference.set_uuid(Some(owner.entity_uuid));
        projectile
    };
    projectile.get_entity().set_velocity(velocity);
    let id = projectile.get_entity().entity_id;
    if world.spawn_entity(projectile) {
        id
    } else {
        -1
    }
}

pub unsafe extern "C" fn projectile_owner(raw: abi::EntityRef) -> i32 {
    // SAFETY: the entity is borrowed for this call.
    unsafe { entity(raw) }
        .and_then(EntityBase::get_owner_entity)
        .map_or(-1, |owner| owner.get_entity().entity_id)
}

pub unsafe extern "C" fn is_attackable(raw: abi::EntityRef) -> bool {
    // SAFETY: the entity is borrowed for this call.
    unsafe { entity(raw) }.is_some_and(|base| {
        base.get_living_entity().is_some_and(LivingEntity::is_alive) && base.can_hit()
    })
}

pub unsafe extern "C" fn can_freeze(raw: abi::EntityRef) -> bool {
    // SAFETY: the entity is borrowed for this call.
    unsafe { entity(raw) }.is_some_and(|base| base.get_entity().can_freeze(base))
}

pub unsafe extern "C" fn shulker_bullet_target(raw: abi::EntityRef) -> i32 {
    use pumpkin_core::entity::projectile::shulker_bullet::ShulkerBulletEntity;
    // SAFETY: the entity is borrowed for this call.
    let Some(bullet) = (unsafe { entity(raw) })
        .and_then(|base| base.cast_any().downcast_ref::<ShulkerBulletEntity>())
    else {
        return -1;
    };
    let target = bullet.target_id();
    if target >= 0
        && bullet
            .entity
            .world
            .load()
            .get_entity_by_id(target)
            .is_some()
    {
        target
    } else {
        -1
    }
}

fn inventory_stack(
    base: &dyn EntityBase,
    slot: u32,
) -> Option<pumpkin_data::item_stack::ItemStack> {
    use pumpkin_inventory::Inventory;
    if slot > 40 {
        return None;
    }
    let stack = base.get_player()?.inventory().get_stack(slot as usize);
    (!stack.is_empty()).then_some(stack)
}

pub unsafe extern "C" fn player_inventory_item(
    raw: abi::EntityRef,
    slot: u32,
    out: abi::Sink,
) -> bool {
    // SAFETY: the entity is borrowed for the active callback.
    let Some(stack) = (unsafe { entity(raw) }).and_then(|base| inventory_stack(base, slot)) else {
        return false;
    };
    let key = stack.item.registry_key;
    let id = if key.contains(':') {
        key.to_owned()
    } else {
        format!("minecraft:{key}")
    };
    // SAFETY: the ID lives until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(id.as_bytes())) };
    true
}

pub unsafe extern "C" fn player_inventory_stack(
    raw: abi::EntityRef,
    slot: u32,
    out: abi::Sink,
) -> bool {
    // SAFETY: the entity is borrowed for the active callback.
    let Some(saved) = (unsafe { entity(raw) })
        .and_then(|base| inventory_stack(base, slot))
        .and_then(|stack| stack.to_nbt())
    else {
        return false;
    };
    let bytes = Nbt::from(saved).write();
    // SAFETY: the bytes live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

pub unsafe extern "C" fn player_fall_flying(raw: abi::EntityRef) -> bool {
    // SAFETY: borrowed entity for this call.
    unsafe { entity(raw) }
        .and_then(EntityBase::get_player)
        .is_some_and(|p| p.get_entity().is_fall_flying())
}

pub unsafe extern "C" fn set_player_fall_flying(raw: abi::EntityRef, flying: bool) -> bool {
    // SAFETY: borrowed entity for this call.
    let Some(player) = (unsafe { entity(raw) }).and_then(EntityBase::get_player) else {
        return false;
    };
    crate::player_actions::set_glide(player.entity_id(), flying);
    if flying && !player.get_entity().is_fall_flying() {
        player.get_entity().set_fall_flying(true);
    } else if !flying && player.get_entity().is_fall_flying() {
        player.get_entity().stop_fall_flying();
    }
    true
}

pub unsafe extern "C" fn player_flight_boost(raw: abi::EntityRef, ticks: u32) -> bool {
    // SAFETY: borrowed entity for this call.
    let Some(player) = (unsafe { entity(raw) }).and_then(EntityBase::get_player) else {
        return false;
    };
    crate::player_actions::flight_boost(player, ticks)
}

pub unsafe extern "C" fn player_selected_slot(raw: abi::EntityRef) -> i32 {
    // SAFETY: borrowed entity for this call.
    unsafe { entity(raw) }
        .and_then(EntityBase::get_player)
        .map_or(-1, |p| i32::from(p.inventory().get_selected_slot()))
}

pub unsafe extern "C" fn can_move_by(raw: abi::EntityRef, delta: abi::Vec3) -> bool {
    // SAFETY: entity is borrowed for this call.
    let (Some(base), Some(delta)) = (unsafe { entity(raw) }, finite(delta)) else {
        return false;
    };
    let e = base.get_entity();
    let world = e.world.load();
    let bounds = e.bounding_box.load().shift(delta);
    world.no_collision(base, bounds)
        && world
            .worldborder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .collision_shape_contains(&bounds)
}

pub unsafe extern "C" fn player_item_cooldown(raw: abi::EntityRef, item: Str, ticks: u32) -> bool {
    // SAFETY: entity and item are borrowed for this call.
    let (Some(player), Ok(ticks)) = (
        (unsafe { entity(raw) }).and_then(EntityBase::get_player),
        i32::try_from(ticks),
    ) else {
        return false;
    };
    // SAFETY: the item string is borrowed for this call.
    let item = unsafe { item.str() };
    if player.has_item_cooldown(item) {
        return false;
    }
    if ticks > 0 {
        player.set_item_cooldown(item, ticks);
    }
    true
}

pub unsafe extern "C" fn player_damage_held_item(raw: abi::EntityRef, amount: u32) -> bool {
    // SAFETY: entity is borrowed for this call.
    let (Some(player), Ok(amount)) = (
        (unsafe { entity(raw) }).and_then(EntityBase::get_player),
        i32::try_from(amount),
    ) else {
        return false;
    };
    player.damage_held_item(amount)
}

pub unsafe extern "C" fn player_reset_attack(raw: abi::EntityRef) -> bool {
    // SAFETY: entity is borrowed for this call.
    let Some(player) = (unsafe { entity(raw) }).and_then(EntityBase::get_player) else {
        return false;
    };
    player.last_attacked_ticks.store(0, Ordering::Relaxed);
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
    if HANGING.contains(&entity.entity_type) {
        flags |= abi::ENTITY_HANGING;
    }
    // ArmorStandEntity.can_hit is false only for a marker while the stand is alive.
    let marker = entity.entity_type == &EntityType::ARMOR_STAND && alive && !base.can_hit();
    if marker || PUSH_IGNORED.contains(&entity.entity_type) {
        flags |= abi::ENTITY_PUSH_IGNORED;
    }
    if entity.no_physics.load(Ordering::Relaxed) {
        flags |= abi::ENTITY_NO_PHYSICS;
    }
    if entity.get_vehicle().is_some() {
        flags |= abi::ENTITY_PASSENGER;
    }
    if entity.is_in_water() {
        flags |= abi::ENTITY_IN_WATER;
    }
    if base.on_climbable() {
        flags |= abi::ENTITY_ON_CLIMBABLE;
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

/// The server never simulates a player's movement, so read `xxa` and `zza` off the keys their
/// client last sent.
fn player_input(player: &Player) -> Vector3<f64> {
    let keys = player.last_input.load(Ordering::Relaxed);
    let axis = |positive, negative| {
        f64::from(i8::from(keys & positive != 0) - i8::from(keys & negative != 0))
    };
    Vector3::new(
        axis(SPlayerInput::LEFT, SPlayerInput::RIGHT),
        0.0,
        axis(SPlayerInput::FORWARD, SPlayerInput::BACKWARD),
    )
}

pub unsafe extern "C" fn body(raw: abi::EntityRef, out: *mut abi::EntityBody) -> bool {
    // SAFETY: the entity is live for the callback.
    let (Some(base), false) = (unsafe { entity(raw) }, out.is_null()) else {
        return false;
    };
    let bounds = base.get_entity().bounding_box.load();
    let input = base.get_player().map_or_else(
        || {
            base.get_living_entity()
                .map_or_else(Vector3::default, |living| living.movement_input.load())
        },
        player_input,
    );
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

pub unsafe extern "C" fn players(raw: WorldRef, out: *mut i32, capacity: usize) -> usize {
    // SAFETY: the world is borrowed for this call.
    let Some(world) = (unsafe { world(raw) }) else {
        return 0;
    };
    let players = world.players.load();
    if !out.is_null() {
        for (index, player) in players.iter().take(capacity).enumerate() {
            // SAFETY: index is below the capacity of the caller's array.
            unsafe { out.add(index).write(player.entity_id()) };
        }
    }
    players.len()
}

pub unsafe extern "C" fn entities_matching(
    raw: WorldRef,
    min: abi::Vec3,
    max: abi::Vec3,
    except: i32,
    exclude: u32,
    skip: usize,
    out: *mut i32,
    capacity: usize,
) -> usize {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(min), Some(max)) = (unsafe { world(raw) }, finite(min), finite(max))
    else {
        return 0;
    };
    let mut seen = 0;
    let mut count = 0;
    for found in world.get_all_at_box(&BoundingBox::new(min, max)) {
        let id = found.get_entity().entity_id;
        let base: &dyn EntityBase = found.as_ref();
        // SAFETY: `base` is live for this call.
        let excluded = unsafe { flags((&raw const base).cast_mut().cast()) } & exclude != 0;
        if id == except || excluded {
            continue;
        }
        seen += 1;
        if seen <= skip {
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

pub unsafe extern "C" fn spawn_carrier(
    raw: WorldRef,
    kind: u8,
    position: abi::Vec3,
    key: Str,
    nbt: Str,
) -> i32 {
    // SAFETY: the world is borrowed for this call.
    let (Some(world), Some(position)) = (unsafe { world(raw) }.and_then(shared), finite(position))
    else {
        return -1;
    };
    let kind = match kind {
        abi::CARRIER_MARKER => &EntityType::MARKER,
        abi::CARRIER_SEAT => &EntityType::ITEM_DISPLAY,
        _ => return -1,
    };
    if world
        .get_block_state_id_if_loaded(&BlockPos::floored_v(position))
        .is_none()
    {
        return -1;
    }
    let carrier = from_type(kind, position, &world, Uuid::new_v4());
    // SAFETY: the caller borrows the key and NBT for this call.
    if key.len != 0 && !unsafe { store_data(carrier.get_entity(), key, nbt) } {
        return -1;
    }
    let id = carrier.get_entity().entity_id;
    if world.spawn_entity(carrier) { id } else { -1 }
}

pub unsafe extern "C" fn discard(raw: abi::EntityRef) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    let entity = base.get_entity();
    if base.get_player().is_some() || entity.removed.load(Ordering::Acquire) {
        return false;
    }
    entity.remove();
    true
}

/// Vanilla `Entity.startRiding(vehicle, force)`. Pumpkin's `add_passenger` leaves its checks and
/// the dismount from an earlier vehicle to the caller.
pub unsafe extern "C" fn start_riding(raw: abi::EntityRef, vehicle: i32, force: bool) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    let rider = base.get_entity();
    let world = rider.world.load();
    let (Some(rider_arc), Some(vehicle)) = (
        world.get_entity_by_id(rider.entity_id),
        world.get_entity_by_id(vehicle),
    ) else {
        return false;
    };
    let target = vehicle.get_entity();
    let rides_target = |entity: &Entity| {
        entity
            .get_vehicle()
            .is_some_and(|current| current.get_entity().entity_id == target.entity_id)
    };
    if target.entity_id == rider.entity_id || rides_target(rider) {
        return false;
    }
    // Vanilla refuses a vehicle that already rides this entity, directly or further down.
    let mut below = target.get_vehicle();
    for _ in 0..MAX_RIDE_DEPTH {
        let Some(next) = below else { break };
        if next.get_entity().entity_id == rider.entity_id {
            return false;
        }
        below = next.get_entity().get_vehicle();
    }
    // Entity.canRide is !isShiftKeyDown(); Entity.canAddPassenger is an empty passenger list.
    if !force && (rider.sneaking.load(Ordering::Relaxed) || !riders(target).is_empty()) {
        return false;
    }
    if let Some(current) = rider.get_vehicle() {
        current.get_entity().remove_passenger(rider.entity_id);
    }
    target.add_passenger(vehicle.clone(), rider_arc);
    rides_target(rider)
}

pub unsafe extern "C" fn stop_riding(raw: abi::EntityRef) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    let rider = base.get_entity();
    let Some(vehicle) = rider.get_vehicle() else {
        return false;
    };
    vehicle.get_entity().remove_passenger(rider.entity_id);
    true
}

pub unsafe extern "C" fn vehicle(raw: abi::EntityRef) -> i32 {
    // SAFETY: the entity is live for the callback.
    unsafe { entity(raw) }
        .and_then(|base| base.get_entity().get_vehicle())
        .map_or(0, |vehicle| vehicle.get_entity().entity_id)
}

pub unsafe extern "C" fn has_indirect_passenger(raw: abi::EntityRef, other: i32) -> bool {
    // SAFETY: the entity is live for the callback.
    let Some(base) = (unsafe { entity(raw) }) else {
        return false;
    };
    let mut pending = vec![(riders(base.get_entity()), 0)];
    while let Some((passengers, depth)) = pending.pop() {
        for passenger in passengers {
            let passenger = passenger.get_entity();
            if passenger.entity_id == other {
                return true;
            }
            if depth < MAX_RIDE_DEPTH {
                pending.push((riders(passenger), depth + 1));
            }
        }
    }
    false
}

fn riders(entity: &Entity) -> Vec<Arc<dyn EntityBase>> {
    entity
        .passengers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// A mod data key: a vanilla `ResourceLocation` path, so it can't collide with odd NBT names.
fn data_key<'a>(key: Str) -> Option<&'a str> {
    if key.len == 0 || key.len > abi::MAX_ENTITY_DATA_KEY {
        return None;
    }
    // SAFETY: the caller borrows the key for this call.
    let key = unsafe { key.str() };
    key.bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/' | b':'))
        .then_some(key)
}

/// Reads the abilities flag vanilla `Player.mayBuild()` returns.
pub unsafe extern "C" fn may_build(raw: abi::EntityRef) -> bool {
    // SAFETY: the entity is live for the callback.
    unsafe { entity(raw) }
        .and_then(EntityBase::get_player)
        .is_some_and(|player| {
            player
                .abilities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .allow_modify_world
        })
}

pub unsafe extern "C" fn data(raw: abi::EntityRef, key: Str, out: abi::Sink) -> bool {
    // SAFETY: the entity is live for the callback.
    let (Some(entity), Some(key)) = (unsafe { entity(raw) }, data_key(key)) else {
        return false;
    };
    let saved = entity
        .get_entity()
        .custom_data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_compound(key)
        .cloned();
    let Some(saved) = saved else {
        return false;
    };
    let bytes = Nbt::from(saved).write();
    // SAFETY: the bytes are live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

pub unsafe extern "C" fn set_data(raw: abi::EntityRef, key: Str, nbt: Str) -> bool {
    // SAFETY: the entity is live for the callback; the caller borrows key and NBT for it.
    unsafe { entity(raw) }.is_some_and(|base| unsafe { store_data(base.get_entity(), key, nbt) })
}

/// Stores Java named-root NBT under a mod data key, or removes the key for empty NBT.
///
/// # Safety
/// `key` and `nbt` must be live for the call.
unsafe fn store_data(entity: &Entity, key: Str, nbt: Str) -> bool {
    let Some(key) = data_key(key) else {
        return false;
    };
    let saved = if nbt.len == 0 {
        None
    } else {
        if nbt.len > abi::MAX_ENTITY_DATA_BYTES {
            return false;
        }
        // SAFETY: the caller borrows the bytes for this call.
        let bytes = unsafe { nbt.bytes() };
        let mut reader = Cursor::new(bytes);
        let Ok(nbt) = Nbt::read(&mut NbtReadHelperJava::new(&mut reader)) else {
            return false;
        };
        if reader.position() != bytes.len() as u64 {
            return false;
        }
        Some(nbt.root_tag)
    };
    let mut data = entity
        .custom_data
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match saved {
        Some(saved) => {
            data.put_compound(key, saved);
        }
        None => {
            data.child_tags.remove(key);
        }
    }
    true
}
