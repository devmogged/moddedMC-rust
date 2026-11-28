//! A transactional chest handler for externally mutable inventories. The trial player
//! and cursor commit follow Pumpkin's `native_menu` implementation; the vanilla click
//! machinery itself is shared via `ScreenHandler`.
use std::{
    any::Any,
    sync::{Arc, Mutex, TryLockError, atomic::Ordering},
};

use moddedmc_api::abi::MENU_ROUTE_PLAYER_FALLBACK;
use pumpkin_data::{
    data_component_impl::{BundleContentsImpl, EquipmentSlot},
    item_stack::ItemStack,
    screen::WindowType,
};
use pumpkin_inventory::{
    SimpleInventory,
    entity_equipment::EntityEquipment,
    inventory::Inventory,
    native_menu::{SlotAccess, ValidateMenu},
    player::player_inventory::PlayerInventory,
    screen_handler::{
        InventoryPlayer, MenuAction, ScreenHandler, ScreenHandlerBehaviour, SlotClickEffect,
        SlotClickEffects,
    },
    slot::{NormalSlot, Slot},
};
use pumpkin_protocol::java::server::play::SlotActionType;

use crate::lock;

pub(super) struct State {
    pub stacks: Vec<ItemStack>,
    pub access: Vec<SlotAccess>,
    pub controls: u64,
}
pub(super) type View = Arc<Mutex<Option<State>>>;
pub(super) type Rule = dyn Fn(u8, &ItemStack) -> bool + Send + Sync;
pub(super) type Route = dyn Fn(&ItemStack) -> u64 + Send + Sync;
type Persist = dyn Fn(&dyn InventoryPlayer, &[ItemStack]) -> bool + Send + Sync;

struct MenuSlot {
    slot: NormalSlot,
    index: u8,
    access: Arc<Mutex<Vec<SlotAccess>>>,
    accepts: Arc<Rule>,
    limit: u8,
}

impl Slot for MenuSlot {
    fn get_inventory(&self) -> Arc<dyn Inventory> {
        self.slot.get_inventory()
    }
    fn get_index(&self) -> usize {
        self.slot.get_index()
    }
    fn set_id(&self, id: usize) {
        self.slot.set_id(id);
    }
    fn mark_dirty(&self) {
        self.slot.mark_dirty();
    }
    fn can_insert(&self, stack: &ItemStack) -> bool {
        lock(&self.access)[usize::from(self.index)].insert && (self.accepts)(self.index, stack)
    }
    fn get_max_item_count(&self) -> u8 {
        let native = self.slot.get_max_item_count();
        if self.limit == 0 {
            native
        } else {
            native.min(self.limit)
        }
    }
    fn can_take_items(&self, _: &dyn InventoryPlayer) -> bool {
        lock(&self.access)[usize::from(self.index)].take
    }
    fn allow_modification(&self, _: &dyn InventoryPlayer) -> bool {
        let access = lock(&self.access)[usize::from(self.index)];
        access.take || access.insert
    }
}

struct TrialScreen {
    behaviour: ScreenHandlerBehaviour,
    count: usize,
    route: Arc<Route>,
}

impl TrialScreen {
    fn insert_routed(&self, stack: &mut ItemStack, mask: u64) -> bool {
        let mut moved = false;
        if stack.is_stackable() {
            for (index, slot) in self.behaviour.slots[..self.count].iter().enumerate() {
                if stack.is_empty() {
                    break;
                }
                if mask & (1 << index) == 0 || !slot.can_insert(stack) {
                    continue;
                }
                let mut target = slot.get_stack();
                if target.is_empty() || !target.are_items_and_components_equal(stack) {
                    continue;
                }
                let amount = stack.item_count.min(
                    slot.get_max_item_count_for_stack(&target)
                        .saturating_sub(target.item_count),
                );
                if amount != 0 {
                    target.increment(amount);
                    stack.decrement(amount);
                    slot.set_stack(target);
                    moved = true;
                }
            }
        }
        if !stack.is_empty() {
            for (index, slot) in self.behaviour.slots[..self.count].iter().enumerate() {
                if mask & (1 << index) != 0 && slot.get_stack().is_empty() && slot.can_insert(stack)
                {
                    let amount = slot
                        .get_max_item_count_for_stack(stack)
                        .min(stack.item_count);
                    if amount != 0 {
                        slot.set_stack(stack.split(amount));
                        slot.mark_dirty();
                        moved = true;
                        break;
                    }
                }
            }
        }
        moved
    }
}

impl ScreenHandler for TrialScreen {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn get_behaviour(&self) -> &ScreenHandlerBehaviour {
        &self.behaviour
    }
    fn get_behaviour_mut(&mut self) -> &mut ScreenHandlerBehaviour {
        &mut self.behaviour
    }
    fn quick_move(&mut self, player: &dyn InventoryPlayer, slot: i32) -> ItemStack {
        let Ok(index) = usize::try_from(slot) else {
            return ItemStack::EMPTY.clone();
        };
        let Some(source) = self.behaviour.slots.get(index).cloned() else {
            return ItemStack::EMPTY.clone();
        };
        let mut stack = source.get_stack();
        if stack.is_empty() || !source.can_take_items(player) {
            return ItemStack::EMPTY.clone();
        }
        let original = stack.clone();
        let Ok(count) = i32::try_from(self.count) else {
            return ItemStack::EMPTY.clone();
        };
        let moved = if index < self.count {
            self.insert_item(&mut stack, count, count + 36, true)
        } else {
            let mask = (self.route)(&stack);
            self.insert_routed(&mut stack, mask)
                || (mask & MENU_ROUTE_PLAYER_FALLBACK != 0
                    && if slot < count + 27 {
                        self.insert_item(&mut stack, count + 27, count + 36, false)
                    } else {
                        self.insert_item(&mut stack, count, count + 27, false)
                    })
        };
        if !moved {
            return ItemStack::EMPTY.clone();
        }
        source.set_stack(if stack.is_empty() {
            ItemStack::EMPTY.clone()
        } else {
            stack
        });
        original
    }
}

struct PlayerSnapshot {
    main: [ItemStack; PlayerInventory::MAIN_SIZE],
    equipment: EntityEquipment,
    selected: u8,
}

impl PlayerSnapshot {
    fn capture(inventory: &PlayerInventory) -> Self {
        let main = inventory
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let equipment = lock(&inventory.entity_equipment).clone();
        Self {
            main,
            equipment,
            selected: inventory.get_selected_slot(),
        }
    }

    fn project(&self, inventory: &PlayerInventory) {
        inventory
            .main_inventory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone_from(&self.main);
        lock(&inventory.entity_equipment).clone_from(&self.equipment);
        inventory
            .selected_slot
            .store(self.selected, Ordering::Relaxed);
    }

    fn commit(
        self,
        inventory: &PlayerInventory,
        expected: &Self,
        persist: impl FnOnce() -> bool,
    ) -> bool {
        let mut equipment = lock(&inventory.entity_equipment);
        // Match Pumpkin's lock order: other operations can hold main before equipment.
        let mut main = match inventory.main_inventory.try_write() {
            Ok(main) => main,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if inventory.get_selected_slot() != expected.selected
            || !same_stacks(&main[..], &expected.main)
            || equipment.equipment.len() != expected.equipment.equipment.len()
            || equipment.equipment.iter().any(|(slot, stack)| {
                expected
                    .equipment
                    .equipment
                    .get(slot)
                    .is_none_or(|other| !same_stack(stack, other))
            })
            || !persist()
        {
            return false;
        }
        *main = self.main;
        *equipment = self.equipment;
        true
    }

    fn record_equipment_changes(
        &self,
        before: &Self,
        inventory: &PlayerInventory,
        effects: &mut Vec<SlotClickEffect>,
    ) {
        for slot in inventory.equipment_slots.values() {
            let stack = self.equipment.get(slot);
            if !same_stack(&before.equipment.get(slot), &stack) {
                effects.push(SlotClickEffect::Equipment(slot.clone(), stack));
            }
        }
        let main_hand = &self.main[usize::from(self.selected)];
        if !same_stack(&before.main[usize::from(before.selected)], main_hand) {
            effects.push(SlotClickEffect::Equipment(
                EquipmentSlot::MAIN_HAND,
                main_hand.clone(),
            ));
        }
    }
}

fn same_stack(left: &ItemStack, right: &ItemStack) -> bool {
    (left.is_empty() && right.is_empty())
        || (left.item_count == right.item_count && left.are_items_and_components_equal(right))
}

fn same_stacks(left: &[ItemStack], right: &[ItemStack]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| same_stack(left, right))
}

pub(super) struct ManagedScreen {
    inner: TrialScreen,
    view: View,
    access: Arc<Mutex<Vec<SlotAccess>>>,
    player_inventory: Arc<PlayerInventory>,
    trial_inventory: Arc<SimpleInventory>,
    trial_player: Arc<PlayerInventory>,
    validity: Arc<ValidateMenu>,
    actions: Vec<(usize, MenuAction)>,
    pending_action: Option<MenuAction>,
    persist: Arc<Persist>,
}

impl ManagedScreen {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        sync_id: u8,
        player: &dyn InventoryPlayer,
        view: View,
        validity: Arc<ValidateMenu>,
        actions: Vec<(usize, MenuAction)>,
        accepts: &Arc<Rule>,
        route: Arc<Route>,
        limits: &[u8],
        persist: Arc<Persist>,
    ) -> Option<Self> {
        let count = lock(&view).as_ref()?.stacks.len();
        let window = match count / 9 {
            1 => WindowType::Generic9x1,
            2 => WindowType::Generic9x2,
            3 => WindowType::Generic9x3,
            4 => WindowType::Generic9x4,
            5 => WindowType::Generic9x5,
            6 => WindowType::Generic9x6,
            _ => return None,
        };
        let player_inventory = player.get_inventory();
        let trial_inventory = Arc::new(SimpleInventory::new(count));
        let trial_player = Arc::new(PlayerInventory::new(
            Arc::new(Mutex::new(EntityEquipment::new())),
            Arc::clone(&player_inventory.equipment_slots),
        ));
        let access = Arc::new(Mutex::new(lock(&view).as_ref()?.access.clone()));
        let mut inner = TrialScreen {
            behaviour: ScreenHandlerBehaviour::new(sync_id, Some(window)),
            count,
            route,
        };
        inner.behaviour.container_slots = count;
        for index in 0..count {
            inner.add_slot(Arc::new(MenuSlot {
                slot: NormalSlot::new(trial_inventory.clone(), index),
                index: u8::try_from(index).ok()?,
                access: Arc::clone(&access),
                accepts: Arc::clone(accepts),
                limit: *limits.get(index)?,
            }));
        }
        let player_slots: Arc<dyn Inventory> = trial_player.clone();
        inner.add_player_slots(&player_slots);
        let menu = Self {
            inner,
            view,
            access,
            player_inventory,
            trial_inventory,
            trial_player,
            validity,
            actions,
            pending_action: None,
            persist,
        };
        menu.refresh();
        Some(menu)
    }

    fn project(&self, state: &State) {
        lock(&self.access).clone_from(&state.access);
        let mut slots = self
            .trial_inventory
            .stacks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (slot, stack) in slots.iter_mut().zip(&state.stacks) {
            if !same_stack(slot, stack) {
                slot.clone_from(stack);
                if let Some(bundle) = slot.get_data_component_mut::<BundleContentsImpl>() {
                    bundle.selected_item = BundleContentsImpl::NO_SELECTED_ITEM_INDEX;
                }
            }
        }
    }

    fn refresh(&self) {
        if let Some(state) = lock(&self.view).as_ref() {
            self.project(state);
        }
        PlayerSnapshot::capture(&self.player_inventory).project(&self.trial_player);
    }
}

impl ScreenHandler for ManagedScreen {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn get_behaviour(&self) -> &ScreenHandlerBehaviour {
        self.inner.get_behaviour()
    }
    fn get_behaviour_mut(&mut self) -> &mut ScreenHandlerBehaviour {
        self.inner.get_behaviour_mut()
    }
    fn take_slot_action(&mut self) -> Option<MenuAction> {
        self.pending_action.take()
    }
    fn can_use(&self, player: &dyn InventoryPlayer) -> bool {
        (self.validity)(player)
    }
    fn quick_move(&mut self, player: &dyn InventoryPlayer, slot: i32) -> ItemStack {
        self.inner.quick_move(player, slot)
    }
    fn send_content_updates(&mut self) {
        self.refresh();
        self.inner.send_content_updates();
    }

    fn set_selected_bundle_item_index(&mut self, slot_index: i32, selected_item_index: i32) {
        let Ok(index) = usize::try_from(slot_index) else {
            return;
        };
        if index < self.inner.count {
            self.inner
                .set_selected_bundle_item_index(slot_index, selected_item_index);
        } else if let Some(slot) = self.inner.get_behaviour().slots.get(index) {
            let mut slots = self
                .player_inventory
                .main_inventory
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(bundle) = slots
                .get_mut(slot.get_index())
                .and_then(|stack| stack.get_data_component_mut::<BundleContentsImpl>())
            {
                bundle.toggle_selected_item(selected_item_index);
            }
        }
    }

    fn on_slot_click(
        &mut self,
        slot: i32,
        button: i32,
        action: SlotActionType,
        player: &dyn InventoryPlayer,
    ) {
        if player.is_spectator() || !self.can_use(player) {
            return;
        }
        if let Ok(index) = usize::try_from(slot)
            && let Some((_, callback)) = self.actions.iter().find(|(slot, _)| *slot == index)
        {
            if matches!(action, SlotActionType::Pickup)
                && button == 0
                && lock(&self.inner.behaviour.cursor_stack).is_empty()
            {
                self.pending_action = Some(Arc::clone(callback));
            }
            return;
        }
        let mut view = lock(&self.view);
        let Some(state) = view.as_mut() else {
            return;
        };
        if *lock(&self.access) != state.access
            || !same_stacks(
                &state.stacks,
                &self
                    .trial_inventory
                    .stacks
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )
        {
            self.project(state);
            self.inner.sync_state();
            return;
        }
        let before = PlayerSnapshot::capture(&self.player_inventory);
        before.project(&self.trial_player);
        let cursor = self.inner.behaviour.cursor_stack.clone();
        let old_cursor = lock(&cursor).clone();
        let mut effects = Vec::new();
        self.inner.on_slot_click_with_effects(
            slot,
            button,
            action,
            player,
            &mut SlotClickEffects::Staged {
                inventory: &self.trial_player,
                effects: &mut effects,
            },
        );
        let proposed = self
            .trial_inventory
            .stacks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = !same_stacks(&state.stacks, &proposed);
        let after = PlayerSnapshot::capture(&self.trial_player);
        after.record_equipment_changes(&before, &self.player_inventory, &mut effects);
        let committed = after.commit(&self.player_inventory, &before, || {
            !changed || (self.persist)(player, &proposed)
        });
        if committed {
            if changed {
                state.stacks.clone_from(&proposed);
            }
        } else {
            *lock(&cursor) = old_cursor;
        }
        drop(proposed);
        drop(view);
        if committed {
            for effect in effects {
                effect.apply(player);
            }
        } else {
            self.refresh();
            self.inner.sync_state();
        }
    }
}
