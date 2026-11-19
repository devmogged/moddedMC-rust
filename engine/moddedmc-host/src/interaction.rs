//! `moddedmc:interaction`: content items used on blocks.

use std::{ptr, sync::Arc};

use moddedmc_api::abi::{self, Str};
use pumpkin_core::{
    block::registry::BlockActionResult,
    entity::{EntityBase, player::Player},
    item::{ItemBehaviour, registry::install_content_behaviour},
    server::Server,
    world::World,
};
use pumpkin_data::{
    Block, BlockDirection, data_component::DataComponent, data_component_impl::CustomDataImpl,
    item_stack::ItemStack,
};
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

use crate::content::use_on_block_callback;

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
};

/// The `custom_data` key that holds a mod's bytes on its item stacks.
const DATA_KEY: &str = "moddedmc:data";

struct ItemUse;

impl ItemBehaviour for ItemUse {
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
        let Some(callback) = use_on_block_callback(item.item.id) else {
            return BlockActionResult::Pass;
        };
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
            abi::USE_SUCCESS => BlockActionResult::Success,
            abi::USE_CONSUME => BlockActionResult::Consume,
            abi::USE_FAIL => BlockActionResult::Fail,
            _ => BlockActionResult::Pass,
        }
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
    pumpkin_inventory::screen_handler::offer_or_drop_stack(player, stack);
    true
}

/// Vanilla `BlockBehaviour.useItemOn` and `useWithoutItem` for content blocks.
pub fn use_block(
    callback: abi::UseBlockFn,
    world: &World,
    player: &Player,
    stack: &mut ItemStack,
    context: &abi::UseBlock,
) -> i32 {
    let entity: &dyn EntityBase = player;
    // SAFETY: the world, player, stack and context outlive this synchronous callback.
    unsafe {
        callback(
            ptr::from_ref(world).cast(),
            ptr::from_ref(&entity).cast(),
            ptr::from_mut(stack).cast(),
            context,
        )
    }
}
