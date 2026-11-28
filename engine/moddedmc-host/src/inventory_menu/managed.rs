//! Managed menus add shared access and external transactions to the legacy menu API.
use std::{ffi::c_void, sync::Arc};

use super::{
    HashMap, IN_CALLBACK, InventoryId, InventoryPlayer, ItemStack, KINDS, Kind, LazyLock, Mutex,
    Nbt, Ordering, Player, Str, TextComponent, Transaction, ValidateMenu, Weak, abi, decode_access,
    encode, initialize_stacks, invoke_callback, lock, menu_actions, ptr, stacks,
};

mod screen;
use screen::{ManagedScreen, Route, Rule, State, View};

type WeakView = Weak<Mutex<Option<State>>>;
static VIEWS: LazyLock<Mutex<HashMap<InventoryId, WeakView>>> = LazyLock::new(Mutex::default);

fn view(id: InventoryId) -> View {
    let mut views = lock(&VIEWS);
    views.retain(|_, view| view.strong_count() != 0);
    views.get(&id).and_then(Weak::upgrade).unwrap_or_else(|| {
        let view = Arc::new(Mutex::new(None));
        views.insert(id, Arc::downgrade(&view));
        view
    })
}

fn validity_check(
    id: InventoryId,
    context: Arc<[u8]>,
    callback: abi::InventoryMenuValidityCallback,
) -> Arc<ValidateMenu> {
    Arc::new(move |player| {
        if IN_CALLBACK.get() {
            return false;
        }
        let Some(player) = player.as_any().downcast_ref::<Player>() else {
            return false;
        };
        let world = player.world();
        if world.uuid != id.0 {
            return false;
        }
        let validity = abi::InventoryMenuValidity {
            player: abi::PlayerRef(ptr::from_ref(player).cast()),
            world: ptr::from_ref(world.as_ref()).cast(),
            inventory: id.2,
            context: Str::new(&context),
        };
        // SAFETY: context and player live until the callback returns.
        invoke_callback(|| unsafe { callback(&raw const validity) })
    })
}

fn item_rule(context: Arc<[u8]>, accepts: abi::InventoryMenuAccept) -> Arc<Rule> {
    Arc::new(move |slot, stack| {
        stack_bytes(stack).is_some_and(|bytes| {
            // SAFETY: both arguments are borrowed during this synchronous predicate.
            invoke_callback(|| unsafe { accepts(Str::new(&context), slot, Str::new(&bytes)) })
        })
    })
}

fn move_route(context: Arc<[u8]>, route: abi::InventoryMenuRoute) -> Arc<Route> {
    Arc::new(move |stack| {
        stack_bytes(stack).map_or(0, |bytes| {
            // SAFETY: both arguments are borrowed during this synchronous routing callback.
            invoke_callback(|| unsafe { route(Str::new(&context), Str::new(&bytes)) })
        })
    })
}

pub(super) unsafe extern "C" fn open(raw: *const abi::InventoryMenuManagedOpen) -> bool {
    open_impl(raw, ptr::null())
}

pub(super) unsafe extern "C" fn open_limited(
    raw: *const abi::InventoryMenuManagedOpen,
    limits: *const u8,
) -> bool {
    if limits.is_null() {
        return false;
    }
    open_impl(raw, limits)
}

fn open_impl(raw: *const abi::InventoryMenuManagedOpen, limits: *const u8) -> bool {
    if IN_CALLBACK.get() {
        return false;
    }
    // SAFETY: the caller borrows the request and its nested arrays for this call.
    let Some(raw) = (unsafe { raw.as_ref() }) else {
        return false;
    };
    let request = &raw.base;
    if raw.world.is_null()
        || raw.context.len > abi::MAX_MENU_VALIDITY_CONTEXT
        || !(9..=54).contains(&request.count)
        || !request.count.is_multiple_of(9)
        || request.access.is_null()
        || request.title.len > 256
        || raw.control_count > 54
        || (raw.control_count != 0 && raw.controls.is_null())
    {
        return false;
    }
    // SAFETY: the player handle is borrowed from a live hook.
    let Some(player) = (unsafe { request.player.0.cast::<Player>().as_ref() }) else {
        return false;
    };
    let Some(kind) = lock(&KINDS).get(&request.kind).copied() else {
        return false;
    };
    let Some(callback) = kind.managed else {
        return false;
    };
    let world = player.world();
    if !ptr::eq(raw.world, ptr::from_ref(world.as_ref()).cast()) {
        return false;
    }
    let id = (world.uuid, request.kind, request.inventory);
    // SAFETY: the validated count covers the borrowed access array.
    let Some(access): Option<Vec<_>> =
        (unsafe { std::slice::from_raw_parts(request.access, request.count) })
            .iter()
            .copied()
            .map(decode_access)
            .collect()
    else {
        return false;
    };
    let controls = if raw.control_count == 0 {
        &[]
    } else {
        // SAFETY: non-null control array has the declared length.
        unsafe { std::slice::from_raw_parts(raw.controls, raw.control_count) }
    };
    let Some(actions) = menu_actions(kind, id, &access, controls) else {
        return false;
    };
    let control_mask = actions.iter().fold(0, |mask, (slot, _)| mask | (1 << slot));
    // SAFETY: the caller borrows the immutable context for this call.
    let context: Arc<[u8]> = unsafe { raw.context.bytes() }.into();
    let validity = validity_check(id, Arc::clone(&context), raw.validity);
    if !validity(player) {
        return false;
    }
    let view = view(id);
    {
        let mut current = lock(&view);
        if current.is_none() {
            let Some(stacks) = initialize_stacks(request) else {
                return false;
            };
            *current = Some(State {
                stacks,
                access,
                controls: control_mask,
            });
        }
        if current.as_ref().is_none_or(|state| {
            state.stacks.len() != request.count || state.controls != control_mask
        }) {
            return false;
        }
    }
    let rule = item_rule(Arc::clone(&context), raw.accepts);
    let route = move_route(Arc::clone(&context), raw.quick_move);
    let limits = if limits.is_null() {
        vec![0; request.count]
    } else {
        // SAFETY: the open_limited contract supplies one entry per validated slot.
        unsafe { std::slice::from_raw_parts(limits, request.count) }.to_vec()
    };
    if !Arc::ptr_eq(&world, &player.world()) {
        return false;
    }
    player.increment_screen_handler_sync_id();
    let Some(menu) = ManagedScreen::new(
        player.screen_handler_sync_id.load(Ordering::Relaxed),
        player,
        view,
        validity,
        actions,
        &rule,
        route,
        &limits,
        Arc::new(move |player, stacks| {
            persist_managed(kind, callback, id, &context, player, stacks)
        }),
    ) else {
        return false;
    };
    // SAFETY: title is borrowed through the open request.
    let title = TextComponent::text(unsafe { request.title.str() }.to_owned());
    player.open_handled_screen_direct(Arc::new(Mutex::new(menu)), &title);
    true
}

fn stack_bytes(stack: &ItemStack) -> Option<bytes::Bytes> {
    let bytes = Nbt::from(stack.to_nbt()?).write();
    (bytes.len() <= abi::MAX_STACK_BYTES).then_some(bytes)
}

pub(super) unsafe extern "C" fn mutate(raw: *const abi::InventoryMenuMutation) -> bool {
    if IN_CALLBACK.get() {
        return false;
    }
    // SAFETY: caller borrows the mutation request.
    let Some(raw) = (unsafe { raw.as_ref() }) else {
        return false;
    };
    if !(9..=54).contains(&raw.count)
        || !raw.count.is_multiple_of(9)
        || raw.stacks.is_null()
        || raw.access.is_null()
        || lock(&KINDS)
            .get(&raw.kind)
            .is_none_or(|kind| kind.managed.is_none())
    {
        return false;
    }
    // SAFETY: world is a borrowed handle from the mod's current callback.
    let Some(world) = (unsafe { crate::content::world(raw.world) }) else {
        return false;
    };
    // SAFETY: validated arrays are borrowed for this call.
    let Some(access): Option<Vec<_>> =
        (unsafe { std::slice::from_raw_parts(raw.access, raw.count) })
            .iter()
            .copied()
            .map(decode_access)
            .collect()
    else {
        return false;
    };
    // SAFETY: the request borrows the complete stack array.
    let Some(stacks): Option<Vec<_>> =
        (unsafe { std::slice::from_raw_parts(raw.stacks, raw.count) })
            .iter()
            .copied()
            .map(|stack| {
                if stack.len == 0 {
                    Some(ItemStack::EMPTY.clone())
                } else {
                    stacks::read_stack(stack)
                }
            })
            .collect()
    else {
        return false;
    };
    let view = view((world.uuid, raw.kind, raw.inventory));
    apply_mutation(&view, stacks, access, || {
        // SAFETY: the mod supplies a synchronous guarded write with a live callback context.
        invoke_callback(|| unsafe { (raw.persist)(raw.persist_context) })
    })
}

fn apply_mutation(
    view: &View,
    stacks: Vec<ItemStack>,
    access: Vec<super::SlotAccess>,
    persist: impl FnOnce() -> bool,
) -> bool {
    let mut current = lock(view);
    if current.as_ref().is_some_and(|state| {
        state.stacks.len() != stacks.len()
            || access.iter().enumerate().any(|(slot, access)| {
                state.controls & (1 << slot) != 0 && (access.take || access.insert)
            })
    }) || !persist()
    {
        return false;
    }
    if let Some(state) = current.as_mut() {
        state.stacks = stacks;
        state.access = access;
    }
    true
}

fn persist_managed(
    kind: Kind,
    callback: abi::InventoryMenuManagedCallback,
    id: InventoryId,
    context: &[u8],
    player: &dyn InventoryPlayer,
    proposed: &[ItemStack],
) -> bool {
    let Some(player) = player.as_any().downcast_ref::<Player>() else {
        return false;
    };
    let world = player.world();
    if world.uuid != id.0 {
        return false;
    }
    let Some(bytes) = encode(proposed) else {
        return false;
    };
    let slots: Vec<_> = bytes
        .iter()
        .map(|bytes| bytes.as_deref().map_or(Str::EMPTY, Str::new))
        .collect();
    let raw_world = ptr::from_ref(world.as_ref()).cast();
    let mut transaction = Transaction::new(raw_world, kind.owner);
    let change = abi::InventoryMenuManagedChange {
        base: abi::InventoryMenuChange {
            world: raw_world,
            player_uuid: *player.gameprofile.id.as_bytes(),
            inventory: id.2,
            stacks: slots.as_ptr(),
            count: slots.len(),
            overflow: ptr::NonNull::<Str>::dangling().as_ptr(),
            overflow_count: 0,
            transaction: (&raw mut transaction).cast(),
            commit: Transaction::commit,
        },
        context: Str::new(context),
        commit_external,
    };
    // SAFETY: the change and all its borrowed fields are live for the callback.
    invoke_callback(|| unsafe { callback(&raw const change) });
    transaction.committed
}

unsafe extern "C" fn commit_external(
    transaction: *mut c_void,
    context: *mut c_void,
    persist: abi::InventoryMenuPersist,
) -> bool {
    // SAFETY: this callback borrows the current transaction from persist_managed.
    let Some(transaction) = (unsafe { transaction.cast::<Transaction>().as_mut() }) else {
        return false;
    };
    if transaction.committed {
        return false;
    }
    // SAFETY: the mod borrows its persistence context for this synchronous call.
    transaction.committed = unsafe { persist(context) };
    transaction.committed
}

#[cfg(test)]
mod tests {
    use super::{Arc, ItemStack, Mutex, State, View, apply_mutation, lock};
    use pumpkin_inventory::native_menu::SlotAccess;

    #[test]
    fn external_write_updates_both_slots_and_permissions_only_on_success() {
        let closed = SlotAccess {
            take: false,
            insert: false,
        };
        let open = SlotAccess {
            take: true,
            insert: true,
        };
        let view: View = Arc::new(Mutex::new(Some(State {
            stacks: vec![ItemStack::EMPTY.clone(); 9],
            access: vec![closed; 9],
            controls: 1 << 8,
        })));
        let witness = Arc::clone(&view);
        let mut permissions = vec![open; 9];
        permissions[8] = closed;
        let slots = vec![ItemStack::EMPTY.clone(); 9];
        assert!(!apply_mutation(
            &view,
            slots.clone(),
            permissions.clone(),
            || false
        ));
        assert!(
            lock(&witness)
                .as_ref()
                .is_some_and(|state| state.access[0] == closed)
        );
        assert!(!apply_mutation(&view, slots.clone(), vec![open; 9], || {
            false
        }));
        assert!(apply_mutation(&view, slots, permissions, || true));
        assert!(
            lock(&witness)
                .as_ref()
                .is_some_and(|state| state.access[0] == open && state.access[8] == closed)
        );
        let empty: View = Arc::new(Mutex::new(None));
        assert!(apply_mutation(
            &empty,
            vec![ItemStack::EMPTY.clone(); 9],
            vec![closed; 9],
            || true
        ));
        assert!(lock(&empty).is_none());
    }
}
