//! Save-global, per-world mod data. The host persists this through the normal world-save
//! lifecycle, separate from loaded chunks and block entities.

use crate::{
    InterfaceUnavailable, RegistrationError, Registry, abi, content::World, require_interface,
};

#[derive(Clone, Copy)]
pub struct SavedData(&'static abi::SavedDataV1);

/// Opaque runtime key, obtained during a successful mod registration. Re-register after restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key(pub(crate) u64);

pub fn require() -> Result<SavedData, InterfaceUnavailable> {
    // SAFETY: moddedmc:saved_data v1 has the complete published table.
    unsafe { require_interface(abi::SAVED_DATA, 1) }.map(SavedData)
}

impl SavedData {
    /// Reserve a mod-local key at registration time, before world ticks.
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        name: &str,
    ) -> Result<Key, RegistrationError> {
        let mut key = 0u64;
        // SAFETY: the registration handle, name and output are live for this call.
        let result =
            unsafe { (self.0.register_key)(registry.reg, abi::Str::from(name), &raw mut key) };
        RegistrationError::from_code(result)?;
        if key == 0 {
            return Err(RegistrationError::InvalidDefinition);
        }
        Ok(Key(key))
    }

    /// Read the value from this world's save, or None if it has never been written.
    /// Call only with a borrowed world from an active callback, not during Mod::start.
    pub fn read(self, world: &World<'_>, key: Key) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut bytes).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the world and sink are borrowed for the call.
        unsafe { (self.0.read)(world.raw(), key.0, sink) }.then_some(bytes)
    }

    /// Replace this save's value in memory. The host persists it on save or shutdown.
    /// Empty bytes are a valid (present) value.
    pub fn write(self, world: &World<'_>, key: Key, data: &[u8]) -> bool {
        // SAFETY: world and bytes are borrowed only during this call.
        unsafe { (self.0.write)(world.raw(), key.0, abi::Str::new(data)) }
    }
}
