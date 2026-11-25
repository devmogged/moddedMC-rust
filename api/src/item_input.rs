//! Let hoppers and droppers offer items to a mod-owned block.

use std::{panic::catch_unwind, sync::OnceLock};

use crate::{
    InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World, require_interface,
};

#[derive(Clone, Copy)]
pub struct ItemInput(&'static abi::ItemInputV1);

pub struct Offer<'a> {
    pub world: World<'a>,
    pub position: Pos,
    pub state: u32,
    pub face: u8,
    /// One item in saved stack NBT.
    pub stack: &'a [u8],
    pub simulate: bool,
}

type Receive = fn(Offer<'_>) -> u32;
static RECEIVE: OnceLock<Receive> = OnceLock::new();

pub fn require() -> Result<ItemInput, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:item_input v1.
    unsafe { require_interface(abi::ITEM_INPUT, 1) }.map(ItemInput)
}

impl ItemInput {
    /// Register one callback for the listed blocks after adding them to the registry.
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        blocks: &[&str],
        receive: Receive,
    ) -> Result<(), RegistrationError> {
        RECEIVE
            .set(receive)
            .map_err(|_| RegistrationError::DuplicateId)?;
        for id in blocks {
            // SAFETY: registration and the id are borrowed for the call.
            RegistrationError::from_code(unsafe {
                (self.0.add)(registry.reg, abi::Str::new(id.as_bytes()), receive_item)
            })?;
        }
        Ok(())
    }
}

unsafe extern "C" fn receive_item(
    raw: abi::WorldRef,
    position: Pos,
    state: u32,
    face: u8,
    stack: abi::Str,
    simulate: bool,
) -> u32 {
    let (Some(world), Some(receive)) = (World::borrowed(raw), RECEIVE.get()) else {
        return 0;
    };
    // SAFETY: the host lends the saved stack until this callback returns.
    let stack = unsafe { stack.bytes() };
    catch_unwind(|| {
        receive(Offer {
            world,
            position,
            state,
            face,
            stack,
            simulate,
        })
    })
    .unwrap_or_default()
}
