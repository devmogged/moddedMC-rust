//! `moddedmc:interaction`: item uses on blocks.

use std::{cell::Cell, ptr, sync::Arc};

use moddedmc_api::abi::{self, Str};
use pumpkin_core::{
    block::registry::BlockActionResult,
    entity::{EntityBase, player::Player},
    item::{ContentItemUse, ItemBehaviour, registry::install_content_behaviour},
    server::Server,
    world::World,
};
use pumpkin_data::{
    Block, BlockDirection, data_component::DataComponent, data_component_impl::CustomDataImpl,
    item::Item, item_stack::ItemStack,
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
};

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
        current.item.id == item.id
            && self.content_use(&current, player, hand, ContentItemUse::Start { yaw, pitch })
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
