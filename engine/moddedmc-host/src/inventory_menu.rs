use std::{
    cell::Cell,
    collections::HashMap,
    ptr,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use moddedmc_api::abi::{self, Reg, Str, code};
use pumpkin_core::entity::player::Player;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::{
    native_menu::{MenuInventory, NativeMenu, SlotAccess, ValidateMenu},
    screen_handler::{InventoryPlayer, MenuAction},
};
use pumpkin_nbt::Nbt;
use pumpkin_util::text::TextComponent;
use uuid::Uuid;

use crate::{lock, native, saved_data::Transaction, stacks};

mod managed;

#[derive(Clone, Copy)]
struct Kind {
    owner: &'static str,
    callback: Option<abi::InventoryMenuCallback>,
    managed: Option<abi::InventoryMenuManagedCallback>,
    action: Option<abi::InventoryMenuActionCallback>,
}

#[derive(Clone, Copy, Default)]
pub struct Registration {
    binding: Option<(u64, abi::InventoryMenuCallback)>,
    managed: Option<(u64, abi::InventoryMenuManagedCallback)>,
    legacy_action: Option<abi::InventoryMenuActionCallback>,
    managed_action: Option<abi::InventoryMenuActionCallback>,
    last_managed: bool,
}

type InventoryId = (Uuid, u64, u64);
type OpenInventory = Mutex<Option<LiveInventory>>;

struct LiveInventory {
    slots: Arc<MenuInventory>,
    controls: u64,
}
static NEXT_KIND: AtomicU64 = AtomicU64::new(1);
static KINDS: LazyLock<Mutex<HashMap<u64, Kind>>> = LazyLock::new(Mutex::default);
static INVENTORIES: LazyLock<Mutex<HashMap<InventoryId, Weak<OpenInventory>>>> =
    LazyLock::new(Mutex::default);

thread_local! {
    static IN_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

fn invoke_callback<T>(call: impl FnOnce() -> T) -> T {
    IN_CALLBACK.set(true);
    let result = call();
    IN_CALLBACK.set(false);
    result
}

pub static INVENTORY_MENU: abi::InventoryMenuV1Managed = abi::InventoryMenuV1Managed {
    base: abi::InventoryMenuV1Validity {
        base: abi::InventoryMenuV1Actions {
            base: abi::InventoryMenuV1 {
                size: size_of::<abi::InventoryMenuV1Managed>(),
                register: stage_kind,
                open,
                restore_matching,
            },
            on_action: stage_action,
            open_with_actions,
        },
        open_validated,
    },
    register_managed: stage_managed,
    open_managed: managed::open,
    mutate: managed::mutate,
    open_limited: managed::open_limited,
};

unsafe extern "C" fn stage_kind(
    reg: Reg,
    callback: abi::InventoryMenuCallback,
    out: *mut u64,
) -> i32 {
    if out.is_null() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the staging argument and output belong to the current registration.
    let staging = unsafe { native::staging(reg) };
    if staging.inventory_menu.binding.is_some() {
        return code::DUPLICATE_ID;
    }
    let id = NEXT_KIND.fetch_add(1, Ordering::Relaxed);
    staging.inventory_menu.binding = Some((id, callback));
    staging.inventory_menu.last_managed = false;
    // SAFETY: a successful registration receives the assigned ID in the caller's output.
    unsafe { out.write(id) };
    code::OK
}

unsafe extern "C" fn stage_managed(
    reg: Reg,
    callback: abi::InventoryMenuManagedCallback,
    out: *mut u64,
) -> i32 {
    if out.is_null() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the staging context and output belong to the current registration.
    let staging = &mut unsafe { native::staging(reg) }.inventory_menu;
    if staging.managed.is_some() {
        return code::DUPLICATE_ID;
    }
    let id = NEXT_KIND.fetch_add(1, Ordering::Relaxed);
    staging.managed = Some((id, callback));
    staging.last_managed = true;
    // SAFETY: the caller supplies a valid output for the assigned kind.
    unsafe { out.write(id) };
    code::OK
}

unsafe extern "C" fn stage_action(reg: Reg, callback: abi::InventoryMenuActionCallback) -> i32 {
    // SAFETY: the caller borrows the staging context during registration.
    let staged = &mut unsafe { native::staging(reg) }.inventory_menu;
    if staged.binding.is_none() && staged.managed.is_none() {
        return code::INVALID_DEFINITION;
    }
    let action = if staged.last_managed {
        &mut staged.managed_action
    } else {
        &mut staged.legacy_action
    };
    if action.is_some() {
        return code::DUPLICATE_ID;
    }
    *action = Some(callback);
    code::OK
}

pub fn register(owner: &'static str, registration: Registration) {
    if let Some((id, callback)) = registration.binding {
        lock(&KINDS).insert(
            id,
            Kind {
                owner,
                callback: Some(callback),
                managed: None,
                action: registration.legacy_action,
            },
        );
    }
    if let Some((id, callback)) = registration.managed {
        lock(&KINDS).insert(
            id,
            Kind {
                owner,
                callback: None,
                managed: Some(callback),
                action: registration.managed_action,
            },
        );
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;

    unsafe extern "C" fn legacy(_: *const abi::InventoryMenuChange) {}
    unsafe extern "C" fn managed(_: *const abi::InventoryMenuManagedChange) {}
    unsafe extern "C" fn legacy_action(_: *const abi::InventoryMenuAction) {}
    unsafe extern "C" fn managed_action(_: *const abi::InventoryMenuAction) {}

    #[test]
    fn legacy_and_managed_menus_have_distinct_registrations() {
        let registry = crate::Registry::default();
        let mut staging = crate::Staging {
            registry: &registry,
            mod_idx: 0,
            entity_types: Vec::new(),
            blocks: Vec::new(),
            commands: Vec::new(),
            command_trees: Vec::new(),
            on_stopping: None,
            content: Vec::new(),
            content_items: Vec::new(),
            content_hooks: Default::default(),
            mobs: Vec::new(),
            mob_ai: Default::default(),
            saved_keys: Vec::new(),
            player_death: None,
            brewed_potion: None,
            inventory_menu: Registration::default(),
            native_actor: None,
            services: Vec::new(),
            player_actions: Vec::new(),
            equipment: Vec::new(),
            item_destroyed: None,
        };
        let reg = (&raw mut staging).cast();
        let mut legacy_id = 0;
        let mut managed_id = 0;
        // SAFETY: reg points to the live staging record and outputs are writable.
        unsafe {
            assert_eq!(stage_action(reg, legacy_action), code::INVALID_DEFINITION);
            assert_eq!(stage_kind(reg, legacy, &raw mut legacy_id), code::OK);
            assert_eq!(stage_action(reg, legacy_action), code::OK);
            assert_eq!(stage_managed(reg, managed, &raw mut managed_id), code::OK);
            assert_eq!(stage_action(reg, managed_action), code::OK);
            assert_eq!(
                stage_kind(reg, legacy, &raw mut legacy_id),
                code::DUPLICATE_ID
            );
            assert_eq!(
                stage_managed(reg, managed, &raw mut managed_id),
                code::DUPLICATE_ID
            );
        }
        assert_ne!(legacy_id, managed_id);
        register("test:mixed_menu", staging.inventory_menu);
        let mut kinds = lock(&KINDS);
        let legacy_kind = kinds.remove(&legacy_id).unwrap();
        let managed_kind = kinds.remove(&managed_id).unwrap();
        assert!(legacy_kind.callback.is_some() && legacy_kind.managed.is_none());
        assert!(managed_kind.callback.is_none() && managed_kind.managed.is_some());
        assert!(std::ptr::fn_addr_eq(
            legacy_kind.action.unwrap(),
            legacy_action as abi::InventoryMenuActionCallback
        ));
        assert!(std::ptr::fn_addr_eq(
            managed_kind.action.unwrap(),
            managed_action as abi::InventoryMenuActionCallback
        ));
    }
}

const fn decode_access(access: u8) -> Option<SlotAccess> {
    if access & !(abi::MENU_TAKE | abi::MENU_INSERT) != 0 {
        return None;
    }
    Some(SlotAccess {
        take: access & abi::MENU_TAKE != 0,
        insert: access & abi::MENU_INSERT != 0,
    })
}

unsafe extern "C" fn open(request: *const abi::InventoryMenuOpen) -> bool {
    // SAFETY: the caller borrows the request for the duration of this call.
    unsafe { request.as_ref() }.is_some_and(|request| open_menu(request, None, &[], None))
}

unsafe extern "C" fn open_with_actions(
    request: *const abi::InventoryMenuOpen,
    world: abi::WorldRef,
    controls: *const abi::InventoryMenuControl,
    count: usize,
) -> bool {
    if world.is_null() || count > 54 || (count != 0 && controls.is_null()) {
        return false;
    }
    let controls = if count == 0 {
        &[]
    } else {
        // SAFETY: the caller borrows the complete controls array for this call.
        unsafe { std::slice::from_raw_parts(controls, count) }
    };
    // SAFETY: the caller borrows the request for the duration of this call.
    unsafe { request.as_ref() }
        .is_some_and(|request| open_menu(request, Some(world), controls, None))
}

unsafe extern "C" fn open_validated(
    request: *const abi::InventoryMenuOpen,
    world: abi::WorldRef,
    context: Str,
    callback: abi::InventoryMenuValidityCallback,
) -> bool {
    if world.is_null() || context.len > abi::MAX_MENU_VALIDITY_CONTEXT || IN_CALLBACK.get() {
        return false;
    }
    // SAFETY: the caller borrows the request and context until this call returns.
    let Some(request) = (unsafe { request.as_ref() }) else {
        return false;
    };
    let context = unsafe { context.bytes() }.to_vec();
    open_menu(request, Some(world), &[], Some((context, callback)))
}

fn open_menu(
    request: &abi::InventoryMenuOpen,
    expected_world: Option<abi::WorldRef>,
    controls: &[abi::InventoryMenuControl],
    validation: Option<(Vec<u8>, abi::InventoryMenuValidityCallback)>,
) -> bool {
    if IN_CALLBACK.get() {
        return false;
    }
    // SAFETY: the request borrows the host's player for this call.
    let Some(player) = (unsafe { request.player.0.cast::<Player>().as_ref() }) else {
        return false;
    };
    if !(9..=54).contains(&request.count)
        || !request.count.is_multiple_of(9)
        || request.access.is_null()
        || request.title.len > 256
    {
        return false;
    }
    let Some(kind) = lock(&KINDS).get(&request.kind).copied() else {
        return false;
    };
    if kind.callback.is_none() {
        return false;
    }
    // SAFETY: the request borrows the complete array and title for this call.
    let access = unsafe { std::slice::from_raw_parts(request.access, request.count) };
    let Some(access): Option<Vec<_>> = access.iter().copied().map(decode_access).collect() else {
        return false;
    };
    let world = player.world();
    if expected_world
        .is_some_and(|expected| !ptr::eq(expected, ptr::from_ref(world.as_ref()).cast()))
    {
        return false;
    }
    let id = (world.uuid, request.kind, request.inventory);
    let validity: Option<Arc<ValidateMenu>> = validation.map(|(context, callback)| {
        let check: Arc<ValidateMenu> = Arc::new(move |player| {
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
            // SAFETY: player, world and owned context live through this synchronous call.
            invoke_callback(|| unsafe { callback(&raw const validity) })
        });
        check
    });
    if validity.as_ref().is_some_and(|check| !check(player)) {
        return false;
    }
    let Some(actions) = menu_actions(kind, id, &access, controls) else {
        return false;
    };
    let controls = actions.iter().fold(0, |mask, (slot, _)| mask | (1 << slot));
    let view = {
        let mut open = lock(&INVENTORIES);
        open.retain(|_, inventory| inventory.strong_count() != 0);
        open.get(&id).and_then(Weak::upgrade).unwrap_or_else(|| {
            let inventory = Arc::new(Mutex::new(None));
            open.insert(id, Arc::downgrade(&inventory));
            inventory
        })
    };
    let inventory = {
        let mut current = lock(&view);
        if current.is_none() {
            *current = initialize_inventory(request).map(|slots| LiveInventory {
                slots: Arc::new(slots),
                controls,
            });
        }
        let Some(inventory) = current.as_ref().filter(|inventory| {
            inventory.slots.size() == access.len() && inventory.controls == controls
        }) else {
            return false;
        };
        Arc::clone(&inventory.slots)
    };
    if expected_world.is_some() && !Arc::ptr_eq(&world, &player.world()) {
        return false;
    }
    player.increment_screen_handler_sync_id();
    let Some(mut menu) = NativeMenu::new(
        player.screen_handler_sync_id.load(Ordering::Relaxed),
        player,
        inventory,
        access,
        Arc::new(move |player, proposed, overflow| {
            let _keep_view_alive = &view;
            persist(kind, id, player, proposed, overflow)
        }),
    ) else {
        return false;
    };
    if let Some(validity) = validity {
        menu.set_validity(validity);
    }
    for (index, action) in actions {
        if !menu.set_slot_action(index, action) {
            return false;
        }
    }
    // SAFETY: the title bytes are borrowed from the caller through this call.
    let title = TextComponent::text(unsafe { request.title.str() }.to_owned());
    player.open_handled_screen_direct(Arc::new(Mutex::new(menu)), &title);
    true
}

fn initialize_inventory(request: &abi::InventoryMenuOpen) -> Option<MenuInventory> {
    MenuInventory::new(initialize_stacks(request)?)
}

fn initialize_stacks(request: &abi::InventoryMenuOpen) -> Option<Vec<ItemStack>> {
    let mut initial = vec![Str::EMPTY; request.count];
    // SAFETY: the caller keeps its initializer alive until open returns.
    if !invoke_callback(|| unsafe {
        (request.initialize)(
            request.initialize_context,
            initial.as_mut_ptr(),
            initial.len(),
        )
    }) {
        return None;
    }
    initial
        .into_iter()
        .map(|stack| {
            if stack.len == 0 {
                Some(ItemStack::EMPTY.clone())
            } else {
                stacks::read_stack(stack)
            }
        })
        .collect()
}

fn menu_actions(
    kind: Kind,
    id: InventoryId,
    access: &[SlotAccess],
    controls: &[abi::InventoryMenuControl],
) -> Option<Vec<(usize, MenuAction)>> {
    let mut seen = [false; 54];
    controls
        .iter()
        .map(|control| {
            let index = usize::from(control.slot);
            let slot = access.get(index)?;
            if slot.take || slot.insert || std::mem::replace(&mut seen[index], true) {
                return None;
            }
            let callback = kind.action?;
            let tag = control.tag;
            let action: MenuAction = Arc::new(move |player| {
                let Some(player) = player.as_any().downcast_ref::<Player>() else {
                    return;
                };
                let world = player.world();
                if world.uuid != id.0 {
                    return;
                }
                let action = abi::InventoryMenuAction {
                    player: abi::PlayerRef(ptr::from_ref(player).cast()),
                    world: ptr::from_ref(world.as_ref()).cast(),
                    inventory: id.2,
                    tag,
                };
                // SAFETY: player, world and action context live through this synchronous callback.
                unsafe { callback(&raw const action) };
            });
            Some((index, action))
        })
        .collect()
}

unsafe extern "C" fn restore_matching(
    player: abi::PlayerRef,
    kind: u64,
    inventory: u64,
    targets: *const abi::InventoryMenuTarget,
    count: usize,
) -> bool {
    if IN_CALLBACK.get() {
        return false;
    }
    // SAFETY: the caller borrows the player and target array for this call.
    let Some(player) = (unsafe { player.0.cast::<Player>().as_ref() }) else {
        return false;
    };
    if !(1..=54).contains(&count) || targets.is_null() {
        return false;
    }
    let id = (player.world().uuid, kind, inventory);
    let Some(view) = lock(&INVENTORIES).get(&id).and_then(Weak::upgrade) else {
        return false;
    };
    let Some(inventory) = lock(&view)
        .as_ref()
        .map(|inventory| Arc::clone(&inventory.slots))
    else {
        return false;
    };
    let handler = lock(&player.current_screen_handler).clone();
    let mut handler = lock(&handler);
    let Some(menu) = handler.as_any_mut().downcast_mut::<NativeMenu>() else {
        return false;
    };
    // SAFETY: the caller borrows the non-null array of validated length through this call.
    let targets: Vec<_> = unsafe { std::slice::from_raw_parts(targets, count) }
        .iter()
        .map(|target| {
            (
                usize::from(target.menu_slot),
                (target.player_slot != abi::MENU_ANY_SLOT)
                    .then_some(usize::from(target.player_slot)),
            )
        })
        .collect();
    menu.is_view_of(&inventory) && menu.restore_matching(player, &targets)
}

fn encode(stacks: &[ItemStack]) -> Option<Vec<Option<bytes::Bytes>>> {
    stacks
        .iter()
        .map(|stack| {
            if stack.is_empty() {
                Some(None)
            } else {
                let bytes = Nbt::from(stack.to_nbt()?).write();
                (bytes.len() <= abi::MAX_STACK_BYTES).then_some(Some(bytes))
            }
        })
        .collect()
}

fn persist(
    kind: Kind,
    id: InventoryId,
    player: &dyn InventoryPlayer,
    stacks: &[ItemStack],
    overflow: &[ItemStack],
) -> bool {
    let Some(callback) = kind.callback else {
        return false;
    };
    let Some(player) = player.as_any().downcast_ref::<Player>() else {
        return false;
    };
    let world = player.world();
    if world.uuid != id.0 {
        return false;
    }
    let (Some(bytes), Some(overflow_bytes)) = (encode(stacks), encode(overflow)) else {
        return false;
    };
    let stacks: Vec<_> = bytes
        .iter()
        .map(|bytes| bytes.as_deref().map_or(Str::EMPTY, Str::new))
        .collect();
    let overflow: Vec<_> = overflow_bytes
        .iter()
        .flatten()
        .map(|bytes| Str::new(bytes))
        .collect();
    let raw_world = ptr::from_ref(world.as_ref()).cast();
    let mut transaction = Transaction::new(raw_world, kind.owner);
    let change = abi::InventoryMenuChange {
        world: raw_world,
        player_uuid: *player.gameprofile.id.as_bytes(),
        inventory: id.2,
        stacks: stacks.as_ptr(),
        count: stacks.len(),
        overflow: overflow.as_ptr(),
        overflow_count: overflow.len(),
        transaction: (&raw mut transaction).cast(),
        commit: Transaction::commit,
    };
    // SAFETY: the world, trial stacks and transaction live through this synchronous callback.
    invoke_callback(|| unsafe { callback(&raw const change) });
    transaction.committed
}
