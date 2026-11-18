//! Loaded dimensions and the dimension of a command source.

use crate::{Command, InterfaceUnavailable, abi, require_interface};

#[derive(Clone, Copy)]
pub struct World(&'static abi::WorldV1);

pub fn require() -> Result<World, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:world v1.
    unsafe { require_interface(abi::WORLD, 1) }.map(World)
}

impl World {
    pub fn dimensions(self) -> impl Iterator<Item = &'static str> {
        // SAFETY: dimension names are static.
        (0..)
            .map(move |index| unsafe { (self.0.dimension)(index).str() })
            .take_while(|name| !name.is_empty())
    }

    pub fn command_dimension(self, command: &Command<'_>) -> Option<&'static str> {
        // SAFETY: the source is live for the call and dimension names are static.
        let name = unsafe { (self.0.command_dimension)(command.source).str() };
        (!name.is_empty()).then_some(name)
    }
}
