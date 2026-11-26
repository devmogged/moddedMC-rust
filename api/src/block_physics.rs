//! Override custom block hardness, explosion resistance, tool requirement and sound.
use crate::{InterfaceUnavailable, RegistrationError, Registry, abi, require_interface, s};

#[derive(Clone, Copy)]
pub struct BlockPhysics(&'static abi::BlockPhysicsV1);

pub fn require() -> Result<BlockPhysics, InterfaceUnavailable> {
    // SAFETY: this is the published version-one table.
    unsafe { require_interface(abi::BLOCK_PHYSICS, 1) }.map(BlockPhysics)
}

impl BlockPhysics {
    pub fn set(
        self,
        registry: &mut Registry<'_>,
        block: &str,
        hardness: f32,
        blast_resistance: f32,
        tool_required: bool,
        sound_from: &str,
    ) -> Result<(), RegistrationError> {
        // SAFETY: registration and both strings live through the call.
        RegistrationError::from_code(unsafe {
            (self.0.set)(
                registry.reg,
                s(block),
                hardness,
                blast_resistance,
                tool_required,
                s(sound_from),
            )
        })
    }
}
