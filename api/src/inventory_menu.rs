//! Stock inventory screens for mod-owned items.

use std::{ffi::c_void, sync::OnceLock};

use crate::{
    InterfaceUnavailable, Player, RegistrationError, Registry, abi,
    content::{self, World},
    guard, require_interface,
    saved_data::Key,
};

pub use crate::abi::{InventoryMenuTarget as Target, MENU_ANY_SLOT as ANY_PLAYER_SLOT};

#[derive(Clone, Copy)]
pub struct Menus(&'static abi::InventoryMenuV1);

#[derive(Clone, Copy)]
pub struct MenuKind(u64);

#[derive(Clone, Copy)]
pub struct Access {
    pub take: bool,
    pub insert: bool,
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

pub type OnChange = fn(Change<'_>);
static ON_CHANGE: OnceLock<OnChange> = OnceLock::new();

pub fn require() -> Result<Menus, InterfaceUnavailable> {
    content::require()?;
    // SAFETY: this is the named versioned C table.
    unsafe { require_interface(abi::INVENTORY_MENU, 1) }.map(Menus)
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
        let access: Vec<_> = access
            .iter()
            .map(|slot| {
                (u8::from(slot.take) * abi::MENU_TAKE) | (u8::from(slot.insert) * abi::MENU_INSERT)
            })
            .collect();
        let mut pending = InitialSlots {
            load: Some(load),
            stacks: Vec::new(),
        };
        let open = abi::InventoryMenuOpen {
            player: player.raw,
            kind: kind.0,
            inventory,
            title: abi::Str::from(title),
            access: access.as_ptr(),
            count: access.len(),
            initialize_context: (&raw mut pending).cast(),
            initialize: initialize::<F>,
        };
        // SAFETY: all inputs are borrowed for the call.
        unsafe { (self.0.open)(&raw const open) }
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
