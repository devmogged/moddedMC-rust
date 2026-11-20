//! Typed access to immutable, versioned cross-mod C tables.

use std::ffi::c_void;

use crate::{
    InterfaceUnavailable, RegistrationError, Registry, abi, require_interface,
    require_interface_prefix,
};

#[derive(Clone, Copy)]
pub struct Services(&'static abi::ServicesV1);

/// Require the host's registry to publish services from this module.
pub fn require_host() -> Result<Services, InterfaceUnavailable> {
    // SAFETY: moddedmc:services v1 has a stable repr(C) layout and a byte-size header.
    unsafe { require_interface(abi::SERVICES, 1) }.map(Services)
}

impl Services {
    /// Publish a pointer to a permanently allocated, immutable repr(C) C interface.
    /// # Safety
    /// The table must have a leading usize size field, match its published ABI,
    /// and remain readable and valid until this process exits.
    pub unsafe fn publish(
        self,
        registration: &mut Registry<'_>,
        name: &str,
        version: u32,
        table: *const c_void,
    ) -> Result<(), RegistrationError> {
        // SAFETY: the caller guarantees the pointer is valid for the process lifetime.
        let result =
            unsafe { (self.0.publish)(registration.reg, abi::Str::from(name), version, table) };
        RegistrationError::from_code(result)
    }
}

/// Require an independently published service during Mod::start.
/// # Safety
/// T must be the exact repr(C) table type for the name/version, beginning
/// with a usize size. It must remain a process-lifetime table.
pub unsafe fn require<T>(
    name: &'static str,
    version: u32,
) -> Result<&'static T, InterfaceUnavailable> {
    // SAFETY: the caller identifies the published table and requires all fields.
    unsafe { require_interface(name, version) }
}

/// Require only the published prefix, then size-check optional appended fields.
/// # Safety
/// T must be the exact repr(C) type for the named service.
pub unsafe fn require_prefix<T>(
    name: &'static str,
    version: u32,
    min_size: usize,
) -> Result<&'static T, InterfaceUnavailable> {
    // SAFETY: the caller promises T matches the published service ABI.
    unsafe { require_interface_prefix(name, version, min_size) }
}
