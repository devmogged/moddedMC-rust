//! `moddedmc:containers` v1: vanilla block containers with `NeoForge` `IItemHandler` semantics,
//! from `InvWrapper`, `SidedInvWrapper` and `VanillaHopperItemHandler` (`NeoForge` 21.1).

use std::sync::Arc;

use moddedmc_api::abi::{self, NO_FACE, Pos, Sink, Str, WorldRef};
use pumpkin_core::block::entities::{
    blasting_furnace::BlastingFurnaceBlockEntity, brewing_stand::BrewingStandBlockEntity,
    furnace::FurnaceBlockEntity, furnace_like_block_entity::CookingBlockEntityBase,
    hopper::HopperBlockEntity, smoker::SmokerBlockEntity,
};
use pumpkin_data::{
    Block, BlockDirection, block_properties::FurnaceLikeProperties, item::Item,
    item_stack::ItemStack,
};
use pumpkin_inventory::inventory::Inventory;
use pumpkin_nbt::Nbt;

use crate::{
    content::{block_pos, in_world_tick, shared, world},
    stacks::read_stack,
};

/// The container behind `NeoForge`'s `Capabilities.ItemHandler.BLOCK` and the side it was asked for.
struct Handler {
    container: Arc<dyn Inventory>,
    side: Option<BlockDirection>,
}

/// `SidedInvWrapper` covers a `WorldlyContainer`, `InvWrapper` everything else.
fn handler(raw: WorldRef, position: Pos, face: u8) -> Option<Handler> {
    let side = if face == NO_FACE {
        None
    } else {
        Some(*BlockDirection::all().get(usize::from(face))?)
    };
    // SAFETY: the world is live for the call.
    let world = shared(unsafe { world(raw) }?)?;
    let container = HopperBlockEntity::get_block_container(&world, &block_pos(position))?;
    Some(Handler { container, side })
}

impl Handler {
    fn is<T: 'static>(&self) -> bool {
        self.container.as_any().is::<T>()
    }

    /// The side a `SidedInvWrapper` filters by.
    fn face(&self) -> Option<BlockDirection> {
        self.side.filter(|_| self.container.is_sided())
    }

    fn slots(&self) -> usize {
        self.face().map_or_else(
            || self.container.size(),
            |side| self.container.get_slots_for_face(side).len(),
        )
    }

    /// `SidedInvWrapper.getSlot`: the container slot for a handler slot.
    fn slot(&self, slot: u32) -> Option<usize> {
        let slot = usize::try_from(slot).ok()?;
        self.face().map_or_else(
            || (slot < self.container.size()).then_some(slot),
            |side| self.container.get_slots_for_face(side).get(slot).copied(),
        )
    }

    /// `getSlotLimit` for a container slot; `NeoForge` caps brewing stand bottles at one.
    fn slot_limit(&self, slot: usize) -> u8 {
        if self.container.is_sided() && self.is::<BrewingStandBlockEntity>() && slot < 3 {
            1
        } else {
            self.container.get_max_count_per_stack()
        }
    }

    /// `canPlaceItem`, plus `canPlaceItemThroughFace` for a `WorldlyContainer`.
    fn can_place(&self, slot: usize, stack: &ItemStack) -> bool {
        (!self.container.is_sided()
            || self
                .container
                .can_place_item_through_face(slot, stack, self.side))
            && self.container.is_valid_slot_for(slot, stack)
    }

    /// How many of `stack` fit in `current`: the shared arithmetic of `insertItem`.
    fn fits(&self, slot: usize, current: &ItemStack, stack: &ItemStack) -> u8 {
        let limit = self.slot_limit(slot);
        let room = if current.is_empty() {
            let furnace = self.is::<FurnaceBlockEntity>()
                || self.is::<BlastingFurnaceBlockEntity>()
                || self.is::<SmokerBlockEntity>();
            // `NeoForge`'s `newStackInsertLimit`: one bucket in a furnace's fuel slot.
            if self.container.is_sided() && furnace && slot == 1 && stack.item.id == Item::BUCKET.id
            {
                1
            } else {
                stack.get_max_stack_size().min(limit)
            }
        } else if current.item_count >= current.get_max_stack_size().min(limit)
            || !stack.are_items_and_components_equal(current)
        {
            return 0;
        } else {
            stack
                .get_max_stack_size()
                .min(limit)
                .saturating_sub(current.item_count)
        };
        if self.can_place(slot, stack) {
            room.min(stack.item_count)
        } else {
            0
        }
    }

    /// `insertItem`, returning the remainder.
    fn insert(&self, slot: u32, mut stack: ItemStack, simulate: bool) -> ItemStack {
        let Some(slot) = self.slot(slot).filter(|_| !stack.is_empty()) else {
            return stack;
        };
        if simulate {
            let moved = self.fits(slot, &self.container.get_stack(slot), &stack);
            stack.decrement(moved);
            return stack;
        }
        let hopper = self.container.as_any().downcast_ref::<HopperBlockEntity>();
        let was_empty = hopper.is_some_and(Inventory::is_empty);
        let mut moved = 0;
        self.container.update_stack(slot, &mut |current| {
            moved = self.fits(slot, current, &stack);
            if moved == 0 {
                return false;
            }
            if current.is_empty() {
                *current = stack.copy_with_count(moved);
            } else {
                current.increment(moved);
            }
            true
        });
        if moved > 0 {
            self.container.mark_dirty();
            // `VanillaHopperItemHandler`: filling an empty hopper starts its transfer cooldown.
            if let (Some(hopper), true) = (hopper, was_empty) {
                hopper.set_cooldown_after_outside_insert();
            }
        }
        stack.decrement(moved);
        stack
    }

    /// `ItemHandlerHelper.insertItemStacked`.
    fn insert_stacked(&self, mut stack: ItemStack, simulate: bool) -> ItemStack {
        if stack.is_empty() {
            return stack;
        }
        let slots = u32::try_from(self.slots()).unwrap_or(u32::MAX);
        let stack_in = |slot: u32| {
            self.slot(slot).map_or_else(
                || ItemStack::EMPTY.clone(),
                |slot| self.container.get_stack(slot),
            )
        };
        if stack.is_stackable() {
            for slot in 0..slots {
                if stack_in(slot).are_items_and_components_equal(&stack) {
                    stack = self.insert(slot, stack, simulate);
                    if stack.is_empty() {
                        return stack;
                    }
                }
            }
        }
        // A stack that doesn't stack goes straight to `insertItem` over every slot, which only an
        // empty slot accepts.
        for slot in 0..slots {
            if !stack.is_stackable() || stack_in(slot).is_empty() {
                stack = self.insert(slot, stack, simulate);
                if stack.is_empty() {
                    break;
                }
            }
        }
        stack
    }

    /// `extractItem`.
    fn extract(&self, slot: u32, amount: u32, simulate: bool) -> ItemStack {
        let (Some(slot), true) = (self.slot(slot), amount > 0) else {
            return ItemStack::EMPTY.clone();
        };
        let current = self.container.get_stack(slot);
        let blocked = self.face().is_some_and(|side| {
            !self
                .container
                .can_take_item_through_face(slot, &current, side)
        });
        if current.is_empty() || blocked {
            return ItemStack::EMPTY.clone();
        }
        let amount = u8::try_from(amount)
            .unwrap_or(u8::MAX)
            .min(current.item_count);
        if simulate {
            return current.copy_with_count(amount);
        }
        let taken = self.container.remove_stack_specific(slot, amount);
        self.container.mark_dirty();
        taken
    }
}

/// Sends a stack to the sink as saved NBT; an empty stack sends nothing.
fn put(stack: &ItemStack, out: &Sink) {
    if let Some(saved) = stack.to_nbt() {
        let bytes = Nbt::from(saved).write();
        // SAFETY: the bytes are live until the synchronous sink returns.
        unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    }
}

unsafe extern "C" fn slots(raw: WorldRef, position: Pos, face: u8) -> i32 {
    handler(raw, position, face).map_or(-1, |handler| {
        i32::try_from(handler.slots()).unwrap_or(i32::MAX)
    })
}

unsafe extern "C" fn stack(raw: WorldRef, position: Pos, face: u8, slot: u32, out: Sink) -> bool {
    let Some(handler) = handler(raw, position, face) else {
        return false;
    };
    let Some(stack) = handler
        .slot(slot)
        .map(|slot| handler.container.get_stack(slot))
        .filter(|stack| !stack.is_empty())
    else {
        return false;
    };
    put(&stack, &out);
    true
}

unsafe extern "C" fn slot_limit(raw: WorldRef, position: Pos, face: u8, slot: u32) -> u32 {
    handler(raw, position, face)
        .and_then(|handler| Some(u32::from(handler.slot_limit(handler.slot(slot)?))))
        .unwrap_or(0)
}

unsafe extern "C" fn insert(
    raw: WorldRef,
    position: Pos,
    face: u8,
    slot: u32,
    stack: Str,
    simulate: bool,
    out: Sink,
) -> bool {
    let (Some(handler), Some(stack)) = (handler(raw, position, face), read_stack(stack)) else {
        return false;
    };
    put(&handler.insert(slot, stack, simulate), &out);
    true
}

unsafe extern "C" fn insert_stacked(
    raw: WorldRef,
    position: Pos,
    face: u8,
    stack: Str,
    simulate: bool,
    out: Sink,
) -> bool {
    let (Some(handler), Some(stack)) = (handler(raw, position, face), read_stack(stack)) else {
        return false;
    };
    put(&handler.insert_stacked(stack, simulate), &out);
    true
}

unsafe extern "C" fn extract(
    raw: WorldRef,
    position: Pos,
    face: u8,
    slot: u32,
    amount: u32,
    simulate: bool,
    out: Sink,
) -> bool {
    let Some(handler) = handler(raw, position, face) else {
        return false;
    };
    let taken = handler.extract(slot, amount, simulate);
    if taken.is_empty() {
        return false;
    }
    put(&taken, &out);
    true
}

fn heat_counters(furnace: &dyn CookingBlockEntityBase, boost: u16, min_lit: u16) -> (bool, bool) {
    let total = furnace.get_cooking_total_time();
    if total == 0 {
        return (false, false);
    }
    let previous = furnace.get_cooking_time_spent();
    let progress = previous.saturating_add(boost).min(total - 1);
    let advanced = progress > previous;
    if advanced {
        furnace.set_cooking_time_spent(progress);
    }
    let heated = furnace.get_lit_time_remaining() < min_lit;
    if heated {
        furnace.set_lit_time_remaining(min_lit);
        furnace.set_lit_total_time(min_lit);
    }
    (advanced || heated, advanced && progress == total - 1)
}

unsafe extern "C" fn furnace_heat(
    raw: WorldRef,
    position: Pos,
    boost: u16,
    min_lit: u16,
    out: *mut abi::FurnaceHeat,
) -> bool {
    if out.is_null() || ((boost > 0 || min_lit > 0) && !in_world_tick(raw)) {
        return false;
    }
    // SAFETY: the borrowed world is live during the synchronous world callback.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    let pos = block_pos(position);
    let Some(state) = world.get_block_state_id_if_loaded(&pos) else {
        return false;
    };
    let block = Block::from_state_id(state);
    let kind = if block == &Block::FURNACE {
        1
    } else if block == &Block::SMOKER {
        2
    } else if block == &Block::BLAST_FURNACE {
        3
    } else {
        return false;
    };
    let Some(world) = shared(world) else {
        return false;
    };
    let Some(entity) = world.get_block_entity(&pos) else {
        return false;
    };
    let any = entity.as_any();
    let furnace: &dyn CookingBlockEntityBase = match kind {
        1 => match any.downcast_ref::<FurnaceBlockEntity>() {
            Some(f) => f,
            None => return false,
        },
        2 => match any.downcast_ref::<SmokerBlockEntity>() {
            Some(f) => f,
            None => return false,
        },
        _ => match any.downcast_ref::<BlastingFurnaceBlockEntity>() {
            Some(f) => f,
            None => return false,
        },
    };
    let total = furnace.get_cooking_total_time();
    let mut reached_end = false;
    if total > 0 && (boost > 0 || min_lit > 0) {
        let (mut changed, ended) = heat_counters(furnace, boost, min_lit);
        reached_end = ended;
        let mut properties = FurnaceLikeProperties::from_state_id(state);
        if min_lit > 0 && !properties.lit {
            properties.lit = true;
            world.set_block_state(
                &pos,
                properties.to_state_id(block),
                pumpkin_core::world::BlockFlags::NOTIFY_ALL,
            );
            changed = true;
        }
        if changed {
            furnace.mark_dirty();
        }
    }
    // SAFETY: the caller provides a writable result for this call.
    unsafe {
        out.write(abi::FurnaceHeat {
            kind,
            reached_end,
            progress: furnace.get_cooking_time_spent(),
            total,
            lit: furnace.get_lit_time_remaining(),
            lit_duration: furnace.get_lit_total_time(),
        });
    };
    true
}

pub static CONTAINERS: abi::ContainersV1 = abi::ContainersV1 {
    size: size_of::<abi::ContainersV1>(),
    slots,
    stack,
    slot_limit,
    insert,
    insert_stacked,
    extract,
    furnace_heat,
};

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use pumpkin_util::math::position::BlockPos;

    use super::*;

    fn handler(container: impl Inventory + 'static, side: Option<BlockDirection>) -> Handler {
        Handler {
            container: Arc::new(container),
            side,
        }
    }

    #[test]
    fn insert_stacked_tops_up_matching_slots_before_empty_ones() {
        let hopper = HopperBlockEntity::new(BlockPos::new(0, 64, 0));
        hopper.set_stack(3, ItemStack::new(60, &Item::IRON_INGOT));
        let handler = handler(hopper, None);
        let rest = handler.insert_stacked(ItemStack::new(10, &Item::IRON_INGOT), false);
        assert!(rest.is_empty());
        assert_eq!(handler.container.get_stack(3).item_count, 64);
        assert_eq!(handler.container.get_stack(0).item_count, 6);
    }

    #[test]
    fn furnace_heat_clamps_progress_and_preserves_existing_fuel() {
        let furnace = FurnaceBlockEntity::new(BlockPos::new(0, 64, 0));
        furnace.set_cooking_total_time(0);
        assert_eq!(heat_counters(&furnace, 4, 20), (false, false));
        furnace.set_cooking_total_time(200);
        furnace.set_cooking_time_spent(196);
        furnace.set_lit_time_remaining(80);
        furnace.set_lit_total_time(100);
        assert_eq!(heat_counters(&furnace, 4, 20), (true, true));
        assert_eq!(furnace.get_cooking_time_spent(), 199);
        assert_eq!(furnace.get_lit_time_remaining(), 80);
        assert_eq!(furnace.get_lit_total_time(), 100);
        assert_eq!(heat_counters(&furnace, 4, 20), (false, false));
        furnace.set_lit_time_remaining(1);
        assert_eq!(heat_counters(&furnace, 4, 20), (true, false));
        assert_eq!(furnace.get_lit_time_remaining(), 20);
        assert_eq!(furnace.get_lit_total_time(), 20);
    }

    #[test]
    fn furnace_faces_and_fuel_bucket_limit() {
        let furnace = handler(
            FurnaceBlockEntity::new(BlockPos::new(0, 64, 0)),
            Some(BlockDirection::North),
        );
        // A side of a furnace shows only its fuel slot.
        assert_eq!(furnace.slots(), 1);
        let rest = furnace.insert(0, ItemStack::new(16, &Item::BUCKET), true);
        assert_eq!(rest.item_count, 15);
        // The top shows the input slot, which takes anything.
        let top = Handler {
            container: furnace.container.clone(),
            side: Some(BlockDirection::Up),
        };
        assert!(
            top.insert(0, ItemStack::new(16, &Item::BUCKET), false)
                .is_empty()
        );
        assert_eq!(furnace.container.get_stack(0).item_count, 16);
    }

    #[test]
    fn filling_an_empty_hopper_starts_its_cooldown() {
        let handler = handler(HopperBlockEntity::new(BlockPos::new(0, 64, 0)), None);
        assert!(
            handler
                .insert(0, ItemStack::new(1, &Item::IRON_INGOT), false)
                .is_empty()
        );
        let hopper = handler
            .container
            .as_any()
            .downcast_ref::<HopperBlockEntity>()
            .map(|hopper| hopper.cooldown_time.load(Ordering::Relaxed));
        assert_eq!(hopper, Some(8));
    }
}
