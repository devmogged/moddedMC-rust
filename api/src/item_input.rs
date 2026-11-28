//! Let hoppers and droppers offer items to a mod-owned block, and hoppers extract them.

use std::{panic::catch_unwind, sync::OnceLock};

use crate::{
    InterfacePrefix, InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World,
};

#[derive(Clone, Copy)]
pub struct ItemInput(InterfacePrefix<abi::ItemInputV1>);

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
pub struct Take<'a> {
    pub world: World<'a>,
    pub position: Pos,
    pub state: u32,
    pub face: u8,
    pub slot: u32,
    pub expected: Option<&'a [u8]>,
}
type Extract = fn(Take<'_>) -> Option<Vec<u8>>;
static EXTRACT: OnceLock<Extract> = OnceLock::new();

pub fn require() -> Result<ItemInput, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:item_input v1.
    unsafe {
        InterfacePrefix::require(
            abi::ITEM_INPUT,
            1,
            std::mem::offset_of!(abi::ItemInputV1, add_output),
        )
    }
    .map(ItemInput)
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
                ((&raw const (*self.0.ptr).add).read())(
                    registry.reg,
                    abi::Str::new(id.as_bytes()),
                    receive_item,
                )
            })?;
        }
        Ok(())
    }

    pub fn require_output(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(std::mem::offset_of!(abi::ItemInputV1, add_output))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: abi::ITEM_INPUT,
                version: 1,
            })
    }

    pub fn register_output(
        self,
        registry: &mut Registry<'_>,
        block: &str,
        slots: u32,
        take: Extract,
    ) -> Result<(), RegistrationError> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::ItemInputV1, add_output))
            .ok_or(RegistrationError::InvalidDefinition)?;
        EXTRACT
            .set(take)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the extension and registration are live.
        RegistrationError::from_code(unsafe {
            ((*api).add_output)(
                registry.reg,
                abi::Str::new(block.as_bytes()),
                slots,
                extract_item,
            )
        })
    }
}

unsafe extern "C" fn extract_item(
    raw: abi::WorldRef,
    position: Pos,
    state: u32,
    face: u8,
    slot: u32,
    expected: abi::Str,
    out: abi::Sink,
) -> bool {
    let (Some(world), Some(take)) = (World::borrowed(raw), EXTRACT.get()) else {
        return false;
    };
    // SAFETY: the expected stack is borrowed for this call.
    let expected = (expected.len != 0).then(|| unsafe { expected.bytes() });
    let Some(stack) = catch_unwind(|| {
        take(Take {
            world,
            position,
            state,
            face,
            slot,
            expected,
        })
    })
    .ok()
    .flatten() else {
        return false;
    };
    // SAFETY: the host copies the stack before returning.
    unsafe { (out.put)(out.ctx, 0, abi::Str::new(&stack)) };
    true
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_is_an_append_only_extension() {
        assert_eq!(std::mem::offset_of!(abi::ItemInputV1, add), 8);
        assert_eq!(std::mem::offset_of!(abi::ItemInputV1, add_output), 16);
        assert_eq!(std::mem::size_of::<abi::ItemInputV1>(), 24);
        let old = ItemInput(InterfacePrefix {
            ptr: std::ptr::null(),
            size: 16,
        });
        assert!(old.require_output().is_err());
    }
}
