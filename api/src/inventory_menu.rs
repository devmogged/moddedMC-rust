//! Stock inventory screens for mod-owned items.

use std::{ffi::c_void, sync::OnceLock};

use crate::{
    InterfacePrefix, InterfaceUnavailable, Player, RegistrationError, Registry, abi,
    content::{self, World},
    guard, require_interface,
    saved_data::Key,
};

pub use crate::abi::{
    InventoryMenuControl as Control, InventoryMenuTarget as Target,
    MENU_ANY_SLOT as ANY_PLAYER_SLOT,
};

#[derive(Clone, Copy)]
pub struct Menus(&'static abi::InventoryMenuV1);

#[derive(Clone, Copy)]
pub struct Actions(&'static abi::InventoryMenuV1Actions);

#[derive(Clone, Copy)]
pub struct Validated(&'static abi::InventoryMenuV1Validity);

#[derive(Clone, Copy)]
pub struct Managed(InterfacePrefix<abi::InventoryMenuV1Managed>);

#[derive(Clone, Copy)]
pub struct MenuKind(u64);

#[derive(Clone, Copy)]
pub struct Access {
    pub take: bool,
    pub insert: bool,
}

pub struct ActionSlots<'a> {
    pub world: World<'a>,
    pub access: &'a [Access],
    pub controls: &'a [Control],
}

pub struct Action<'a> {
    pub player: Player<'a>,
    pub world: World<'a>,
    pub inventory: u64,
    pub tag: u64,
}

/// Immutable per-view context for checking whether the backing inventory is still accessible.
pub struct ValiditySlots<'a> {
    pub world: World<'a>,
    pub access: &'a [Access],
    pub context: &'a [u8],
}

pub struct ManagedSlots<'a> {
    pub world: World<'a>,
    pub access: &'a [Access],
    pub controls: &'a [Control],
    pub context: &'a [u8],
}

pub struct Validity<'a> {
    pub player: Player<'a>,
    pub world: World<'a>,
    pub inventory: u64,
    pub context: &'a [u8],
}

pub struct Change<'a> {
    pub world: World<'a>,
    pub player_uuid: [u8; 16],
    pub inventory: u64,
    stacks: &'a [abi::Str],
    overflow: &'a [abi::Str],
    raw: &'a abi::InventoryMenuChange,
}

impl<'a> Change<'a> {
    pub fn slots(&self) -> impl ExactSizeIterator<Item = Option<&'a [u8]>> {
        self.stacks.iter().map(|stack| {
            // SAFETY: every stack is borrowed until the callback returns.
            (stack.len != 0).then(|| unsafe { stack.bytes() })
        })
    }

    /// Displaced stacks that did not fit. Save them with the proposed slots on commit.
    pub fn overflow(&self) -> impl ExactSizeIterator<Item = &'a [u8]> {
        // SAFETY: every nonempty overflow stack is borrowed until the callback returns.
        self.overflow.iter().map(|stack| unsafe { stack.bytes() })
    }

    /// Save the mod's replacement record. Only a successful commit applies the click.
    pub fn commit(&mut self, key: Key, value: &[u8]) -> bool {
        // SAFETY: transaction and bytes live for this synchronous call.
        unsafe { (self.raw.commit)(self.raw.transaction, key.0, abi::Str::new(value)) }
    }
}

pub struct ManagedChange<'a> {
    pub change: Change<'a>,
    pub context: &'a [u8],
    raw: &'a abi::InventoryMenuManagedChange,
}

impl ManagedChange<'_> {
    /// Write the backing record while the host holds the inventory transaction lock.
    /// A false return must leave the record unchanged.
    pub fn commit_external<F: FnOnce() -> bool>(&mut self, persist: F) -> bool {
        let mut persist = Some(persist);
        // SAFETY: the host calls the borrowed closure synchronously, at most once.
        unsafe {
            (self.raw.commit_external)(
                self.change.raw.transaction,
                (&raw mut persist).cast(),
                persist_external::<F>,
            )
        }
    }
}

unsafe extern "C" fn persist_external<F: FnOnce() -> bool>(context: *mut c_void) -> bool {
    guard("inventory menu persistence", false, || {
        // SAFETY: the caller borrows a live Option<F> for this synchronous invocation.
        unsafe { &mut *context.cast::<Option<F>>() }
            .take()
            .is_some_and(|persist| persist())
    })
}

pub type OnChange = fn(Change<'_>);
static ON_CHANGE: OnceLock<OnChange> = OnceLock::new();
pub type OnAction = fn(Action<'_>);
static ON_ACTION: OnceLock<OnAction> = OnceLock::new();
pub type OnValidity = fn(Validity<'_>) -> bool;
static ON_VALIDITY: OnceLock<OnValidity> = OnceLock::new();
pub type OnManagedChange = fn(ManagedChange<'_>);
pub type OnAccept = fn(&[u8], u8, &[u8]) -> bool;
pub type OnRoute = fn(&[u8], &[u8]) -> u64;
static ON_MANAGED_CHANGE: OnceLock<OnManagedChange> = OnceLock::new();
static ON_ACCEPT: OnceLock<OnAccept> = OnceLock::new();
static ON_ROUTE: OnceLock<OnRoute> = OnceLock::new();

pub fn require() -> Result<Menus, InterfaceUnavailable> {
    content::require()?;
    // SAFETY: this is the named versioned C table.
    unsafe { require_interface(abi::INVENTORY_MENU, 1) }.map(Menus)
}

pub fn require_actions() -> Result<Actions, InterfaceUnavailable> {
    content::require()?;
    // SAFETY: this checks for the full table including the actions extension.
    unsafe { require_interface(abi::INVENTORY_MENU, 1) }.map(Actions)
}

pub fn require_validity() -> Result<Validated, InterfaceUnavailable> {
    content::require()?;
    // SAFETY: require the append-only table including the validity extension.
    unsafe { require_interface(abi::INVENTORY_MENU, 1) }.map(Validated)
}

pub fn require_managed() -> Result<Managed, InterfaceUnavailable> {
    content::require()?;
    // SAFETY: this is the named v1 table; the required final field covers the full extension.
    unsafe {
        InterfacePrefix::require(
            abi::INVENTORY_MENU,
            1,
            size_of::<abi::InventoryMenuV1Managed>(),
        )
    }
    .map(Managed)
}

impl Managed {
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        on_change: OnManagedChange,
        on_action: OnAction,
        on_validity: OnValidity,
        on_accept: OnAccept,
        on_route: OnRoute,
    ) -> Result<MenuKind, RegistrationError> {
        ON_MANAGED_CHANGE
            .set(on_change)
            .map_err(|_| RegistrationError::DuplicateId)?;
        ON_VALIDITY
            .set(on_validity)
            .map_err(|_| RegistrationError::DuplicateId)?;
        ON_ACCEPT
            .set(on_accept)
            .map_err(|_| RegistrationError::DuplicateId)?;
        ON_ROUTE
            .set(on_route)
            .map_err(|_| RegistrationError::DuplicateId)?;
        let api = self
            .0
            .upto(std::mem::offset_of!(
                abi::InventoryMenuV1Managed,
                register_managed
            ))
            .ok_or(RegistrationError::InvalidDefinition)?;
        let mut kind = 0;
        // SAFETY: the checked table and borrowed registry/output are live.
        RegistrationError::from_code(unsafe {
            ((*api).register_managed)(registry.reg, dispatch_managed, &raw mut kind)
        })?;
        // SAFETY: register_managed lies after the complete actions prefix.
        Actions(unsafe { &(*api).base.base }).on_action(registry, on_action)?;
        Ok(MenuKind(kind))
    }

    pub fn open<F>(
        self,
        player: &Player<'_>,
        kind: MenuKind,
        inventory: u64,
        title: &str,
        slots: ManagedSlots<'_>,
        load: F,
    ) -> bool
    where
        F: FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
    {
        let Some(api) = self.0.upto(std::mem::offset_of!(
            abi::InventoryMenuV1Managed,
            open_managed
        )) else {
            return false;
        };
        if slots.context.len() > abi::MAX_MENU_VALIDITY_CONTEXT {
            return false;
        }
        open_menu(
            player,
            kind,
            inventory,
            title,
            slots.access,
            load,
            |request| {
                // SAFETY: the host borrows this request and copies the view context.
                unsafe {
                    ((*api).open_managed)(&abi::InventoryMenuManagedOpen {
                        base: abi::InventoryMenuOpen { ..*request },
                        world: slots.world.raw(),
                        controls: slots.controls.as_ptr(),
                        control_count: slots.controls.len(),
                        context: abi::Str::new(slots.context),
                        validity: dispatch_validity,
                        accepts: dispatch_accept,
                        quick_move: dispatch_route,
                    })
                }
            },
        )
    }

    /// Replace slots and shared access only if the backing record is committed successfully.
    /// Use a compare-and-write in the persistence closure to reject stale snapshots.
    pub fn mutate<F: FnOnce() -> bool>(
        self,
        world: World<'_>,
        kind: MenuKind,
        inventory: u64,
        access: &[Access],
        stacks: &[Option<Vec<u8>>],
        persist: F,
    ) -> bool {
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::InventoryMenuV1Managed, mutate))
        else {
            return false;
        };
        if access.len() != stacks.len() {
            return false;
        }
        let access: Vec<_> = access.iter().map(encode_access).collect();
        let stacks: Vec<_> = stacks
            .iter()
            .map(|stack| stack.as_deref().map_or(abi::Str::EMPTY, abi::Str::new))
            .collect();
        let mut persist = Some(persist);
        // SAFETY: all borrowed arrays and the synchronous callback context are live.
        unsafe {
            ((*api).mutate)(&abi::InventoryMenuMutation {
                world: world.raw(),
                kind: kind.0,
                inventory,
                stacks: stacks.as_ptr(),
                access: access.as_ptr(),
                count: stacks.len(),
                persist_context: (&raw mut persist).cast(),
                persist: persist_external::<F>,
            })
        }
    }
}

impl Validated {
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        on_change: OnChange,
        on_validity: OnValidity,
    ) -> Result<MenuKind, RegistrationError> {
        ON_VALIDITY
            .set(on_validity)
            .map_err(|_| RegistrationError::DuplicateId)?;
        Menus(&self.0.base.base).register(registry, on_change)
    }

    /// Open a view whose immutable context is copied by the host. The callback must be
    /// read-only, inexpensive, and must not open or mutate a menu.
    pub fn open<F>(
        self,
        player: &Player<'_>,
        kind: MenuKind,
        inventory: u64,
        title: &str,
        slots: ValiditySlots<'_>,
        load: F,
    ) -> bool
    where
        F: FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
    {
        if slots.context.len() > abi::MAX_MENU_VALIDITY_CONTEXT {
            return false;
        }
        open_menu(
            player,
            kind,
            inventory,
            title,
            slots.access,
            load,
            |request| {
                // SAFETY: the host copies the context and borrows the request until open returns.
                unsafe {
                    (self.0.open_validated)(
                        request,
                        slots.world.raw(),
                        abi::Str::new(slots.context),
                        dispatch_validity,
                    )
                }
            },
        )
    }
}

impl Actions {
    pub const fn menus(self) -> Menus {
        Menus(&self.0.base)
    }

    /// Register after registering this mod's inventory menu kind.
    pub fn on_action(
        self,
        registry: &mut Registry<'_>,
        callback: OnAction,
    ) -> Result<(), RegistrationError> {
        ON_ACTION
            .set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the registry and callback belong to this mod's registration.
        RegistrationError::from_code(unsafe { (self.0.on_action)(registry.reg, dispatch_action) })
    }

    /// Open a stock menu with controls in slots that forbid both taking and insertion.
    /// Load their display stacks with the first view's saved slots.
    pub fn open<F>(
        self,
        player: &Player<'_>,
        kind: MenuKind,
        inventory: u64,
        title: &str,
        slots: ActionSlots<'_>,
        load: F,
    ) -> bool
    where
        F: FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
    {
        open_menu(
            player,
            kind,
            inventory,
            title,
            slots.access,
            load,
            |request| {
                // SAFETY: the open request and the action slots are borrowed for this call.
                unsafe {
                    (self.0.open_with_actions)(
                        request,
                        slots.world.raw(),
                        slots.controls.as_ptr(),
                        slots.controls.len(),
                    )
                }
            },
        )
    }
}

impl Menus {
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        on_change: OnChange,
    ) -> Result<MenuKind, RegistrationError> {
        ON_CHANGE
            .set(on_change)
            .map_err(|_| RegistrationError::DuplicateId)?;
        let mut kind = 0;
        // SAFETY: the handle and output are live for this registration.
        RegistrationError::from_code(unsafe {
            (self.0.register)(registry.reg, dispatch, &raw mut kind)
        })?;
        Ok(MenuKind(kind))
    }

    /// Open a one- to six-row screen, nine slots per row. `load` reads the current saved
    /// slots only if there are no active views. Return one stack per slot, including empties.
    /// Access rules are specific to the player opening the view.
    pub fn open<F>(
        self,
        player: &Player<'_>,
        kind: MenuKind,
        inventory: u64,
        title: &str,
        access: &[Access],
        load: F,
    ) -> bool
    where
        F: FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
    {
        open_menu(player, kind, inventory, title, access, load, |request| {
            // SAFETY: the request and its inputs are borrowed for this call.
            unsafe { (self.0.open)(request) }
        })
    }

    /// Recover one active view's slots to original player slots, using the same indices as
    /// the death batch. Use `ANY_PLAYER_SLOT` for loose stacks. The view stays open.
    pub fn restore_matching(
        self,
        player: &Player<'_>,
        kind: MenuKind,
        inventory: u64,
        targets: &[Target],
    ) -> bool {
        // SAFETY: the player and target array are borrowed for this call.
        unsafe {
            (self.0.restore_matching)(
                player.raw,
                kind.0,
                inventory,
                targets.as_ptr(),
                targets.len(),
            )
        }
    }
}

fn open_menu<F>(
    player: &Player<'_>,
    kind: MenuKind,
    inventory: u64,
    title: &str,
    access: &[Access],
    load: F,
    invoke: impl FnOnce(&abi::InventoryMenuOpen) -> bool,
) -> bool
where
    F: FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
{
    let access: Vec<_> = access.iter().map(encode_access).collect();
    let mut pending = InitialSlots {
        load: Some(load),
        stacks: Vec::new(),
    };
    invoke(&abi::InventoryMenuOpen {
        player: player.raw,
        kind: kind.0,
        inventory,
        title: abi::Str::from(title),
        access: access.as_ptr(),
        count: access.len(),
        initialize_context: (&raw mut pending).cast(),
        initialize: initialize::<F>,
    })
}

fn encode_access(slot: &Access) -> u8 {
    (u8::from(slot.take) * abi::MENU_TAKE) | (u8::from(slot.insert) * abi::MENU_INSERT)
}

struct InitialSlots<F> {
    load: Option<F>,
    stacks: Vec<Option<Vec<u8>>>,
}

unsafe extern "C" fn initialize<F>(context: *mut c_void, out: *mut abi::Str, count: usize) -> bool
where
    F: FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
{
    guard("inventory menu initial slots", false, || {
        // SAFETY: the host uses the pending initializer and live output array during open.
        let pending = unsafe { &mut *context.cast::<InitialSlots<F>>() };
        let Some(stacks) = pending.load.take().and_then(|load| load()) else {
            return false;
        };
        if stacks.len() != count {
            return false;
        }
        pending.stacks = stacks;
        let out = unsafe { std::slice::from_raw_parts_mut(out, count) };
        for (out, stack) in out.iter_mut().zip(&pending.stacks) {
            *out = stack.as_deref().map_or(abi::Str::EMPTY, abi::Str::new);
        }
        true
    })
}

unsafe extern "C" fn dispatch(raw: *const abi::InventoryMenuChange) {
    guard("inventory menu change", (), || {
        // SAFETY: the host borrows the context for the duration of this callback.
        let (Some(raw), Some(callback)) = (unsafe { raw.as_ref() }, ON_CHANGE.get()) else {
            return;
        };
        let Some(world) = World::borrowed(raw.world) else {
            return;
        };
        // SAFETY: the host provides a complete borrowed slot array.
        let stacks = unsafe { std::slice::from_raw_parts(raw.stacks, raw.count) };
        let overflow = unsafe { std::slice::from_raw_parts(raw.overflow, raw.overflow_count) };
        callback(Change {
            world,
            player_uuid: raw.player_uuid,
            inventory: raw.inventory,
            stacks,
            overflow,
            raw,
        });
    });
}

unsafe extern "C" fn dispatch_action(raw: *const abi::InventoryMenuAction) {
    guard("inventory menu action", (), || {
        // SAFETY: the host borrows the context for the duration of this callback.
        let (Some(raw), Some(callback)) = (unsafe { raw.as_ref() }, ON_ACTION.get()) else {
            return;
        };
        let (Some(player), Some(world)) = (Player::new(raw.player), World::borrowed(raw.world))
        else {
            return;
        };
        callback(Action {
            player,
            world,
            inventory: raw.inventory,
            tag: raw.tag,
        });
    });
}

unsafe extern "C" fn dispatch_validity(raw: *const abi::InventoryMenuValidity) -> bool {
    guard("inventory menu validity", false, || {
        // SAFETY: all fields, including the copied context, are borrowed for this call.
        let (Some(raw), Some(callback)) = (unsafe { raw.as_ref() }, ON_VALIDITY.get()) else {
            return false;
        };
        let (Some(player), Some(world)) = (Player::new(raw.player), World::borrowed(raw.world))
        else {
            return false;
        };
        callback(Validity {
            player,
            world,
            inventory: raw.inventory,
            context: unsafe { raw.context.bytes() },
        })
    })
}

unsafe extern "C" fn dispatch_managed(raw: *const abi::InventoryMenuManagedChange) {
    guard("managed inventory menu change", (), || {
        // SAFETY: the host borrows the complete extension context for this callback.
        let (Some(raw), Some(callback)) = (unsafe { raw.as_ref() }, ON_MANAGED_CHANGE.get()) else {
            return;
        };
        let base = &raw.base;
        let Some(world) = World::borrowed(base.world) else {
            return;
        };
        // SAFETY: the host provides complete arrays and a context for this synchronous call.
        let stacks = unsafe { std::slice::from_raw_parts(base.stacks, base.count) };
        let overflow = unsafe { std::slice::from_raw_parts(base.overflow, base.overflow_count) };
        callback(ManagedChange {
            change: Change {
                world,
                player_uuid: base.player_uuid,
                inventory: base.inventory,
                stacks,
                overflow,
                raw: base,
            },
            context: unsafe { raw.context.bytes() },
            raw,
        });
    });
}

unsafe extern "C" fn dispatch_accept(context: abi::Str, slot: u8, stack: abi::Str) -> bool {
    guard("inventory menu item predicate", false, || {
        // SAFETY: context and stack are borrowed for this synchronous call.
        ON_ACCEPT
            .get()
            .is_some_and(|callback| unsafe { callback(context.bytes(), slot, stack.bytes()) })
    })
}

unsafe extern "C" fn dispatch_route(context: abi::Str, stack: abi::Str) -> u64 {
    guard("inventory menu quick move route", 0, || {
        // SAFETY: context and stack are borrowed for this synchronous call.
        ON_ROUTE.get().map_or(0, |callback| unsafe {
            callback(context.bytes(), stack.bytes())
        })
    })
}
