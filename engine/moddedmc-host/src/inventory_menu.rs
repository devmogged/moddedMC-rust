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
    native_menu::{MenuInventory, NativeMenu, SlotAccess},
    screen_handler::InventoryPlayer,
};
use pumpkin_nbt::Nbt;
use pumpkin_util::text::TextComponent;
use uuid::Uuid;

use crate::{lock, native, saved_data::Transaction, stacks};

#[derive(Clone, Copy)]
struct Kind {
    owner: &'static str,
    callback: abi::InventoryMenuCallback,
}

type InventoryId = (Uuid, u64, u64);
type OpenInventory = Mutex<Option<Arc<MenuInventory>>>;
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

pub static INVENTORY_MENU: abi::InventoryMenuV1 = abi::InventoryMenuV1 {
    size: size_of::<abi::InventoryMenuV1>(),
    register: stage_kind,
    open,
    restore_matching,
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
    if staging.inventory_menu.is_some() {
        return code::DUPLICATE_ID;
    }
    let id = NEXT_KIND.fetch_add(1, Ordering::Relaxed);
    staging.inventory_menu = Some((id, callback));
    // SAFETY: a successful registration receives the assigned ID in the caller's output.
    unsafe { out.write(id) };
    code::OK
}

pub fn register(owner: &'static str, binding: Option<(u64, abi::InventoryMenuCallback)>) {
    if let Some((id, callback)) = binding {
        lock(&KINDS).insert(id, Kind { owner, callback });
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
    if IN_CALLBACK.get() {
        return false;
    }
    // SAFETY: the caller borrows the request and player for this call.
    let Some(request) = (unsafe { request.as_ref() }) else {
        return false;
    };
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
    // SAFETY: the request borrows the complete array and title for this call.
    let access = unsafe { std::slice::from_raw_parts(request.access, request.count) };
    let Some(access): Option<Vec<_>> = access.iter().copied().map(decode_access).collect() else {
        return false;
    };
    let id = (player.world().uuid, request.kind, request.inventory);
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
        if let Some(inventory) = current.as_ref() {
            if inventory.size() != access.len() {
                return false;
            }
            Arc::clone(inventory)
        } else {
            let mut initial = vec![Str::EMPTY; request.count];
            // SAFETY: the caller keeps its initializer alive until open returns.
            if !invoke_callback(|| unsafe {
                (request.initialize)(
                    request.initialize_context,
                    initial.as_mut_ptr(),
                    initial.len(),
                )
            }) {
                return false;
            }
            let Some(stacks): Option<Vec<_>> = initial
                .into_iter()
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
            let Some(inventory) = MenuInventory::new(stacks) else {
                return false;
            };
            let inventory = Arc::new(inventory);
            *current = Some(Arc::clone(&inventory));
            inventory
        }
    };
    player.increment_screen_handler_sync_id();
    let Some(menu) = NativeMenu::new(
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
    // SAFETY: the title bytes are borrowed from the caller through this call.
    let title = TextComponent::text(unsafe { request.title.str() }.to_owned());
    player.open_handled_screen_direct(Arc::new(Mutex::new(menu)), &title);
    true
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
    let Some(inventory) = lock(&view).clone() else {
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
    invoke_callback(|| unsafe { (kind.callback)(&raw const change) });
    transaction.committed
}
