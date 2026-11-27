//! `moddedmc:interaction`: item uses on blocks.

use std::{
    cell::Cell,
    ptr,
    sync::{Arc, Mutex},
};

use moddedmc_api::abi::{self, Str};
use pumpkin_core::{
    block::registry::BlockActionResult,
    entity::{
        EntityBase,
        player::{Player, install_brewed_potion_hook},
    },
    item::{ContentItemUse, ItemBehaviour, registry::install_content_behaviour},
    server::Server,
    world::World,
};
use pumpkin_data::{
    Block, BlockDirection,
    data_component::DataComponent,
    data_component_impl::{CustomDataImpl, LodestoneTarget, LodestoneTrackerImpl},
    item::Item,
    item_stack::ItemStack,
};
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_protocol::{codec::var_int::VarInt, java::server::play::SUseItemOn};
use pumpkin_util::{
    Hand,
    math::{position::BlockPos, vector3::Vector3},
};

use crate::content::{item_use_callback, use_on_block_callbacks};
use pumpkin_world::clip::{ClipBlock, ClipContext, ClipFluid};

pub static INTERACTION: abi::InteractionV1 = abi::InteractionV1 {
    size: size_of::<abi::InteractionV1>(),
    on_use_on_block: crate::content::on_use_on_block,
    stack_count,
    stack_shrink,
    stack_data,
    set_stack_data,
    cooldown,
    on_use_block: crate::content::on_use_block_content,
    stack_read,
    stack_write,
    give,
    on_player_will_destroy: crate::content::on_player_will_destroy,
    on_redstone: crate::content::on_redstone,
    on_vanilla_use_on_block: crate::content::on_vanilla_use_on_block,
    place_held_block,
    on_item_use: crate::content::on_item_use,
    item_use_animation: crate::content::item_use_animation,
    action_bar,
    on_use_mob: crate::mob_ai::on_use_mob,
    on_melee_damage: crate::content::on_melee_damage,
    on_use_item: crate::content::on_use_item,
    on_player_placed: crate::content::on_player_placed,
    add_exhaustion,
    player_sneaking,
    stack_set_lodestone,
    on_brewed_potion,
};

static BREWED_POTION_CALLBACKS: Mutex<Vec<abi::BrewedPotionFn>> = Mutex::new(Vec::new());

unsafe extern "C" fn on_brewed_potion(reg: abi::Reg, callback: abi::BrewedPotionFn) -> i32 {
    // SAFETY: reg is the active mod registration.
    let slot = &mut unsafe { crate::native::staging(reg) }.brewed_potion;
    if slot.is_some() {
        return abi::code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    abi::code::OK
}

pub fn register_brewed_potion(callback: Option<abi::BrewedPotionFn>) {
    let Some(callback) = callback else { return };
    let mut callbacks = crate::lock(&BREWED_POTION_CALLBACKS);
    if callbacks.is_empty() && !install_brewed_potion_hook(dispatch_brewed_potion) {
        tracing::error!("moddedmc: brewed potion hook is already installed");
        return;
    }
    callbacks.push(callback);
}

fn dispatch_brewed_potion(player: &Player, stack: &ItemStack) {
    let callbacks = crate::lock(&BREWED_POTION_CALLBACKS).clone();
    let Some(saved) = stack
        .to_nbt()
        .map(|nbt| pumpkin_nbt::Nbt::from(nbt).write())
    else {
        return;
    };
    let item = stack.item.identifier();
    let world = player.world();
    let player: &dyn EntityBase = player;
    for callback in callbacks {
        // SAFETY: the world, player and slices are borrowed for the synchronous callback.
        unsafe {
            callback(
                Arc::as_ptr(&world).cast(),
                (&raw const player).cast(),
                Str::new(item.as_bytes()),
                Str::new(&saved),
            );
        }
    }
}

pub static PLAYER_STACK_DATA: abi::PlayerStackDataV1 = abi::PlayerStackDataV1 {
    size: size_of::<abi::PlayerStackDataV1>(),
    compare: compare_player_stack_data,
    compare_lodestone: compare_player_lodestone,
};

unsafe extern "C" fn compare_player_stack_data(
    player: abi::PlayerRef,
    item: Str,
    expected: Str,
    replacement: Str,
    replace: bool,
) -> bool {
    // SAFETY: forwarding the borrowed ABI arguments.
    unsafe { compare_player_stack_inner(player, item, expected, replacement, replace, None) }
}

unsafe extern "C" fn compare_player_lodestone(
    player: abi::PlayerRef,
    item: Str,
    expected: Str,
    replacement: Str,
    dimension: Str,
    pos: abi::Pos,
) -> bool {
    if dimension.len > 256 {
        return false;
    }
    // SAFETY: dimension is borrowed from the caller.
    let Some(tracker) = lodestone_tracker(unsafe { dimension.str() }, pos) else {
        return false;
    };
    // SAFETY: forwarding the borrowed ABI arguments.
    unsafe { compare_player_stack_inner(player, item, expected, replacement, true, Some(tracker)) }
}

fn lodestone_tracker(dimension: &str, pos: abi::Pos) -> Option<LodestoneTrackerImpl> {
    let target = if dimension.is_empty() {
        None
    } else {
        if !dimension.contains(':')
            || pos.x.unsigned_abs() > 30_000_000
            || pos.z.unsigned_abs() > 30_000_000
        {
            return None;
        }
        Some(LodestoneTarget {
            dimension: dimension.into(),
            x: pos.x,
            y: pos.y,
            z: pos.z,
        })
    };
    Some(LodestoneTrackerImpl {
        target,
        tracked: false,
    })
}

fn update_lodestone_tracker(stack: &mut ItemStack, tracker: LodestoneTrackerImpl) {
    if tracker.target.is_some() {
        stack.set_data_component(tracker);
    } else {
        stack.remove_data_component(DataComponent::LodestoneTracker);
    }
}

unsafe fn compare_player_stack_inner(
    player: abi::PlayerRef,
    item: Str,
    expected: Str,
    replacement: Str,
    replace: bool,
    tracker: Option<LodestoneTrackerImpl>,
) -> bool {
    if expected.len == 0
        || expected.len > abi::MAX_STACK_DATA
        || replacement.len > abi::MAX_STACK_DATA
    {
        return false;
    }
    // SAFETY: caller borrows the player and identifiers/data.
    let (Some(player), Some(item)) = (
        unsafe { crate::native::player(player) },
        Item::from_registry_key(unsafe { item.str() }),
    ) else {
        return false;
    };
    // SAFETY: the expected stack data is borrowed for this call.
    let expected = unsafe { expected.bytes() };
    let matches = |stack: &ItemStack| {
        stack.item_count == 1
            && stack.get_item().id == item.id
            && stack
                .get_data_component::<CustomDataImpl>()
                .and_then(|custom| custom.data.get_byte_array(DATA_KEY))
                .is_some_and(|bytes| {
                    bytes.len() == expected.len()
                        && bytes
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| a.cast_unsigned() == *b)
                })
    };
    let inventory = player.inventory();
    // Same lock order as PlayerInventory::swap_item.
    let mut equipment = crate::lock(&inventory.entity_equipment);
    let mut main = crate::write(&inventory.main_inventory);
    let offslot = inventory.equipment_slots.get(&40);
    let mut offhand = offslot.map(|slot| equipment.get(slot));
    let mut indices = main
        .iter()
        .enumerate()
        .filter_map(|(i, s)| matches(s).then_some(i));
    let first = indices.next();
    if indices.next().is_some() {
        return false;
    }
    let off_matches = offhand.as_ref().is_some_and(matches);
    let slot = match (first, off_matches) {
        (Some(i), false) => i,
        (None, true) => 40,
        _ => return false,
    };
    if !replace {
        return true;
    }
    let stack = if slot == 40 {
        let Some(stack) = offhand.as_mut() else {
            return false;
        };
        stack
    } else {
        &mut main[slot]
    };
    // SAFETY: exclusive stack borrow and replacement data are live.
    if !unsafe {
        set_stack_data(
            ptr::from_mut(stack).cast(),
            replacement.ptr,
            replacement.len,
        )
    } {
        return false;
    }
    if let Some(tracker) = tracker {
        update_lodestone_tracker(stack, tracker);
    }
    let updated = stack.clone();
    if slot == 40 {
        let Some(offslot) = offslot else {
            return false;
        };
        equipment.put(offslot, updated.clone());
    }
    drop(main);
    drop(equipment);
    player.sync_hand_slot(slot, updated);
    true
}

unsafe extern "C" fn add_exhaustion(raw: abi::EntityRef, amount: f32) {
    if !amount.is_finite() || amount < 0.0 {
        return;
    }
    // SAFETY: the callback borrows the entity for this call.
    let entity = unsafe { raw.cast::<&dyn EntityBase>().as_ref() };
    if let Some(player) = entity.and_then(|entity| entity.get_player()) {
        player.add_exhaustion(amount);
    }
}

unsafe extern "C" fn player_sneaking(raw: abi::EntityRef) -> bool {
    // SAFETY: the entity is borrowed for the interaction callback.
    unsafe { raw.cast::<&dyn EntityBase>().as_ref() }
        .and_then(|entity| entity.get_player())
        .is_some_and(|player| player.get_entity().is_sneaking())
}

unsafe extern "C" fn action_bar(raw: abi::EntityRef, message: Str) {
    // SAFETY: the callback borrows the entity for this call.
    let Some(player) =
        (unsafe { raw.cast::<&dyn EntityBase>().as_ref() }).and_then(|entity| entity.get_player())
    else {
        return;
    };
    // SAFETY: the message is borrowed for this call.
    let message = unsafe { message.str() };
    player.send_system_message_raw(&pumpkin_util::text::TextComponent::text(message), true);
}

/// The `custom_data` key that holds a mod's bytes on its item stacks.
const DATA_KEY: &str = "moddedmc:data";

struct ItemUse;

impl ItemBehaviour for ItemUse {
    fn use_content_item(
        &self,
        item: &Item,
        player: &Player,
        yaw: f32,
        pitch: f32,
        hand: Hand,
    ) -> bool {
        let current = player.inventory().get_stack_in_hand(hand);
        if current.item.id != item.id || current.is_empty() {
            return false;
        }
        if self.content_use(&current, player, hand, ContentItemUse::Start { yaw, pitch }) {
            return true;
        }
        let Some(callback) = crate::content::use_item_callback(item.id) else {
            return false;
        };
        let inventory = player.inventory();
        let mut stack = current;
        let world = player.world();
        let entity: &dyn EntityBase = player;
        let context = abi::UseItem {
            item: Str::from(item.registry_key),
            yaw,
            pitch,
        };
        // SAFETY: the stack is a local copy and the world, player and context outlive the call.
        let result = unsafe {
            callback(
                ptr::from_ref(world.as_ref()).cast(),
                ptr::from_ref(&entity).cast(),
                ptr::from_mut(&mut stack).cast(),
                &raw const context,
            )
        };
        if matches!(result, abi::USE_SUCCESS | abi::USE_CONSUME) {
            inventory.set_stack_in_hand(hand, stack);
        }
        result != abi::USE_PASS
    }

    fn content_use(
        &self,
        expected: &ItemStack,
        player: &Player,
        hand: Hand,
        event: ContentItemUse,
    ) -> bool {
        let Some(callback) = item_use_callback(expected.item.id) else {
            return false;
        };
        let inventory = player.inventory();
        let mut held = inventory.get_stack_in_hand(hand);
        let start = matches!(event, ContentItemUse::Start { .. });
        if !held.are_items_and_components_equal(expected) {
            // A moved or replaced stack must never complete against its replacement.
            return !start;
        }
        if start
            && player
                .living_entity
                .item_use_time
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
        {
            let active = *player
                .living_entity
                .active_hand
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if active == Some(hand) {
                return true;
            }
        }

        let other_hand = match hand {
            Hand::Right => Hand::Left,
            Hand::Left => Hand::Right,
        };
        let held_before = held.clone();
        let mut other = inventory.get_stack_in_hand(other_hand);
        let other_before = other.clone();
        let world = player.world();
        let (phase, hit) = match event {
            ContentItemUse::Start { yaw, pitch } => {
                let eye = player.eye_position();
                let end = eye.add(
                    &(Vector3::rotation_vector(f64::from(pitch), f64::from(yaw))
                        * player.block_interaction_range()),
                );
                let hit = world
                    .clip(&ClipContext::new(
                        eye,
                        end,
                        ClipBlock::Outline,
                        ClipFluid::None,
                    ))
                    .location;
                (abi::ITEM_USE_START, [hit.x, hit.y, hit.z])
            }
            ContentItemUse::Finish => (abi::ITEM_USE_FINISH, [0.0; 3]),
            ContentItemUse::Release => (abi::ITEM_USE_RELEASE, [0.0; 3]),
        };
        let context = abi::ItemUseContext {
            item: Str::new(held.item.registry_key.as_bytes()),
            hit,
            phase,
            main_hand: hand == Hand::Right,
            creative: player.is_creative(),
        };
        let entity: &dyn EntityBase = player;
        // SAFETY: both stacks are disjoint locals and the other values are borrowed for this call.
        let result = unsafe {
            callback(
                ptr::from_ref(world.as_ref()).cast(),
                ptr::from_ref(&entity).cast(),
                ptr::from_mut(&mut held).cast(),
                ptr::from_mut(&mut other).cast(),
                &raw const context,
            )
        };
        if result == 0 {
            return false;
        }
        if !other.are_items_and_components_equal(&other_before)
            || other.item_count != other_before.item_count
        {
            inventory.set_stack_in_hand(other_hand, other);
        }
        if !held.are_items_and_components_equal(&held_before)
            || held.item_count != held_before.item_count
        {
            inventory.set_stack_in_hand(hand, held.clone());
        }
        if start
            && let Ok(ticks) = u16::try_from(result)
            && ticks != 0
            && !held.is_empty()
        {
            player
                .living_entity
                .set_active_hand(hand, held, i32::from(ticks));
        }
        true
    }

    fn use_on_block(
        &self,
        item: &mut ItemStack,
        player: &Player,
        location: BlockPos,
        face: BlockDirection,
        cursor_pos: Vector3<f32>,
        _block: &Block,
        _server: &Server,
    ) -> BlockActionResult {
        let mut callbacks = use_on_block_callbacks(item.item.id).peekable();
        if callbacks.peek().is_none() {
            return BlockActionResult::Pass;
        }
        let world = player.world();
        let id = item.item.registry_key;
        let context = abi::UseOnBlock {
            item: Str::new(id.as_bytes()),
            position: abi::Pos {
                x: location.0.x,
                y: location.0.y,
                z: location.0.z,
            },
            face: face.to_index(),
            hit: [cursor_pos.x, cursor_pos.y, cursor_pos.z],
            sneaking: player
                .get_entity()
                .sneaking
                .load(std::sync::atomic::Ordering::Relaxed),
            creative: player.is_creative(),
        };
        let entity: &dyn EntityBase = player;
        for callback in callbacks {
            // SAFETY: the world, player, stack and context outlive this synchronous callback.
            let result = unsafe {
                callback(
                    ptr::from_ref(world.as_ref()).cast(),
                    ptr::from_ref(&entity).cast(),
                    ptr::from_mut(item).cast(),
                    &raw const context,
                )
            };
            match result {
                abi::USE_SUCCESS => return BlockActionResult::Success,
                abi::USE_CONSUME => return BlockActionResult::Consume,
                abi::USE_FAIL => return BlockActionResult::Fail,
                _ => {}
            }
        }
        BlockActionResult::Pass
    }

    fn modify_melee_damage(
        &self,
        item: &Item,
        attacker: &Player,
        target: &dyn EntityBase,
        damage: f32,
    ) -> f32 {
        let damage = content_melee_damage(item, target, damage);
        crate::equipment::weapon_hit(item.registry_key, attacker, target, damage)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn content_melee_damage(item: &Item, target: &dyn EntityBase, damage: f32) -> f32 {
    let Some(callback) = crate::content::melee_damage_callback(item.id) else {
        return damage;
    };
    let kind = crate::mobs::registered_kind(target).unwrap_or("");
    // SAFETY: both IDs are borrowed for this synchronous call.
    let adjusted = unsafe {
        callback(
            Str::new(item.registry_key.as_bytes()),
            Str::new(kind.as_bytes()),
            damage,
        )
    };
    if adjusted.is_finite() && adjusted >= 0.0 {
        adjusted
    } else {
        damage
    }
}

pub fn install() -> bool {
    install_content_behaviour(Arc::new(ItemUse))
}

/// # Safety
/// `raw` is the stack a use callback passed, still live and not otherwise borrowed.
const unsafe fn stack<'a>(raw: abi::StackRef) -> Option<&'a mut ItemStack> {
    // SAFETY: the caller's contract.
    unsafe { raw.cast::<ItemStack>().as_mut() }
}

unsafe extern "C" fn stack_count(raw: abi::StackRef) -> u32 {
    // SAFETY: the stack is live for the callback.
    unsafe { stack(raw) }.map_or(0, |stack| u32::from(stack.item_count))
}

unsafe extern "C" fn stack_shrink(raw: abi::StackRef, count: u32) {
    // SAFETY: the stack is live for the callback.
    if let Some(stack) = unsafe { stack(raw) } {
        let count = u8::try_from(count).unwrap_or(u8::MAX);
        stack.item_count = stack.item_count.saturating_sub(count);
    }
}

unsafe extern "C" fn stack_data(raw: abi::StackRef, out: *mut u8, capacity: usize) -> usize {
    // SAFETY: the stack is live for the callback.
    let Some(bytes) = unsafe { stack(raw) }
        .and_then(|stack| stack.get_data_component::<CustomDataImpl>())
        .and_then(|custom| custom.data.get_byte_array(DATA_KEY))
    else {
        return usize::MAX;
    };
    if !out.is_null() {
        for (i, byte) in bytes.iter().take(capacity).enumerate() {
            // SAFETY: `out` has room for `capacity` bytes and `i` is below it.
            unsafe { out.add(i).write(byte.cast_unsigned()) };
        }
    }
    bytes.len()
}

unsafe extern "C" fn set_stack_data(raw: abi::StackRef, data: *const u8, len: usize) -> bool {
    // SAFETY: the stack is live for the callback.
    let Some(stack) = (unsafe { stack(raw) }) else {
        return false;
    };
    if len > abi::MAX_STACK_DATA || (len > 0 && data.is_null()) {
        return false;
    }
    let mut compound = stack
        .get_data_component::<CustomDataImpl>()
        .map_or_else(NbtCompound::new, |custom| custom.data.clone());
    if len == 0 {
        compound.child_tags.remove(DATA_KEY);
    } else {
        // SAFETY: the caller passes `len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        let bytes: Box<[i8]> = bytes.iter().map(|b| b.cast_signed()).collect();
        compound.put(DATA_KEY, NbtTag::ByteArray(bytes));
    }
    if compound.is_empty() {
        stack.remove_data_component(DataComponent::CustomData);
    } else {
        stack.set_data_component(CustomDataImpl::new(compound));
    }
    true
}

unsafe extern "C" fn stack_set_lodestone(
    raw: abi::StackRef,
    data: Str,
    dimension: Str,
    pos: abi::Pos,
) -> bool {
    if dimension.len > 256 {
        return false;
    }
    // SAFETY: the dimension string is borrowed for this call.
    let Some(tracker) = lodestone_tracker(unsafe { dimension.str() }, pos) else {
        return false;
    };
    // SAFETY: the callback exclusively borrows the stack and the data is live.
    if !unsafe { set_stack_data(raw, data.ptr, data.len) } {
        return false;
    }
    // SAFETY: the previous call's stack borrow ended.
    let Some(stack) = (unsafe { stack(raw) }) else {
        return false;
    };
    update_lodestone_tracker(stack, tracker);
    true
}

unsafe extern "C" fn cooldown(player: abi::EntityRef, item: Str, ticks: u32) {
    // SAFETY: the player is live for the callback and the id is borrowed for the call.
    let (Some(entity), item) = (
        unsafe { player.cast::<&dyn EntityBase>().as_ref() },
        unsafe { item.str() },
    ) else {
        return;
    };
    if let Some(player) = entity.get_player() {
        player.start_cooldown(item.to_owned(), i32::try_from(ticks).unwrap_or(i32::MAX));
    }
}

unsafe extern "C" fn stack_read(raw: abi::StackRef, out: abi::Sink) -> bool {
    // SAFETY: the stack is live for the callback.
    let Some(saved) = unsafe { stack(raw) }.and_then(|stack| stack.to_nbt()) else {
        return false;
    };
    let bytes = pumpkin_nbt::Nbt::from(saved).write();
    // SAFETY: the bytes are live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn stack_write(raw: abi::StackRef, nbt: Str) -> bool {
    // SAFETY: the stack is live for the callback.
    let Some(stack) = (unsafe { stack(raw) }) else {
        return false;
    };
    if nbt.len == 0 {
        *stack = ItemStack::EMPTY.clone();
        return true;
    }
    let Some(replacement) = crate::stacks::read_stack(nbt) else {
        return false;
    };
    *stack = replacement;
    true
}

/// The held stack a content block's use callback is changing. Pumpkin hands the callback a copy
/// and writes it back to `hand` afterwards, where vanilla passes the live slot.
#[derive(Clone, Copy)]
struct HandUse {
    player: *const Player,
    hand: Hand,
    stack: *mut ItemStack,
}

thread_local! {
    static HAND_USE: Cell<Option<HandUse>> = const { Cell::new(None) };
}

unsafe extern "C" fn place_held_block(raw: abi::EntityRef, clicked: abi::Pos, face: u8) -> bool {
    // SAFETY: the player is live for this callback.
    let Some(player) = (unsafe { crate::content::entity(raw) }).and_then(EntityBase::get_player)
    else {
        return false;
    };
    let Some(used) = HAND_USE.get().filter(|used| ptr::eq(used.player, player)) else {
        return false;
    };
    let Some(face) = BlockDirection::all().get(usize::from(face)).copied() else {
        return false;
    };
    // SAFETY: HAND_USE keeps the stack borrowed for the outer callback.
    let Some(block) = unsafe { used.stack.as_ref() }
        .filter(|stack| !stack.is_empty())
        .and_then(|stack| Block::from_item_id(stack.item.id))
    else {
        return false;
    };
    let world = player.world();
    let (Some(player), Some(server)) = (
        world.get_player_by_id(player.entity_id()),
        world.server.upgrade(),
    ) else {
        return false;
    };
    let position = crate::content::block_pos(clicked);
    let normal = face.to_offset();
    let center = |delta| match delta {
        -1 => 0.0,
        1 => 1.0,
        _ => 0.5,
    };
    let hit = SUseItemOn {
        hand: VarInt(i32::from(used.hand == Hand::Left)),
        position,
        face: VarInt(i32::from(face.to_index())),
        cursor_pos: Vector3::new(center(normal.x), center(normal.y), center(normal.z)),
        inside_block: false,
        is_against_world_border: false,
        sequence: VarInt(0),
    };
    server
        .block_registry
        .place_block(&player, block, &server, &hit, position, face)
        .is_ok_and(|placed| placed.is_some())
}

unsafe extern "C" fn give(player: abi::EntityRef, nbt: Str) -> bool {
    // SAFETY: the player is live for the callback.
    let (Some(entity), Some(stack)) = (
        unsafe { player.cast::<&dyn EntityBase>().as_ref() },
        crate::stacks::read_stack(nbt),
    ) else {
        return false;
    };
    let Some(player) = entity.get_player() else {
        return false;
    };
    let inventory = player.inventory();
    match HAND_USE.get().filter(|used| ptr::eq(used.player, player)) {
        // Vanilla `Inventory.add` merges into the live held slot, so the copy has to be in the
        // slot while it runs and come back out with the result.
        Some(used) => {
            // SAFETY: the use callback that set HAND_USE is still running and only reaches its
            // stack through the pointer it was given.
            let held = unsafe { &mut *used.stack };
            inventory.set_stack_in_hand(used.hand, held.clone());
            pumpkin_inventory::screen_handler::offer_or_drop_stack(player, stack);
            *held = inventory.get_stack_in_hand(used.hand);
        }
        None => pumpkin_inventory::screen_handler::offer_or_drop_stack(player, stack),
    }
    true
}

/// Vanilla `BlockBehaviour.useItemOn` and `useWithoutItem` for content blocks. `hand` is the
/// hand `stack` is a copy of, if the caller writes it back there.
pub fn use_block(
    callback: abi::UseBlockFn,
    world: &World,
    player: &Player,
    (stack, hand): (&mut ItemStack, Option<Hand>),
    context: &abi::UseBlock,
) -> i32 {
    let entity: &dyn EntityBase = player;
    let stack = ptr::from_mut(stack);
    let previous = HAND_USE.replace(hand.map(|hand| HandUse {
        player: ptr::from_ref(player),
        hand,
        stack,
    }));
    // SAFETY: the world, player, stack and context outlive this synchronous callback.
    let result = unsafe {
        callback(
            ptr::from_ref(world).cast(),
            ptr::from_ref(&entity).cast(),
            stack.cast(),
            context,
        )
    };
    HAND_USE.set(previous);
    result
}

/// Vanilla `Mob.mobInteract` for a mod mob. `stack` is a copy of the stack in `hand`, which the
/// caller writes back.
pub fn use_mob(
    callback: abi::UseMobFn,
    world: &World,
    player: &Player,
    (stack, hand): (&mut ItemStack, Hand),
    mob: &dyn EntityBase,
    context: &abi::UseMob,
) -> i32 {
    let entity: &dyn EntityBase = player;
    let stack = ptr::from_mut(stack);
    let previous = HAND_USE.replace(Some(HandUse {
        player: ptr::from_ref(player),
        hand,
        stack,
    }));
    // SAFETY: the world, player, stack, mob and context outlive this synchronous callback.
    let result = unsafe {
        callback(
            ptr::from_ref(world).cast(),
            ptr::from_ref(&entity).cast(),
            stack.cast(),
            ptr::from_ref(&mob).cast(),
            context,
        )
    };
    HAND_USE.set(previous);
    result
}
