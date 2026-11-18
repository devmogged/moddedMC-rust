//! Shutdown hooks for finishing mod work before the server saves its worlds.

use std::sync::OnceLock;

use crate::{InterfaceUnavailable, RegistrationError, Registry, abi, guard, require_interface};

#[derive(Clone, Copy)]
pub struct Lifecycle(&'static abi::LifecycleV1);

pub fn require() -> Result<Lifecycle, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:lifecycle v1.
    unsafe { require_interface(abi::LIFECYCLE, 1) }.map(Lifecycle)
}

static STOP: OnceLock<fn()> = OnceLock::new();

unsafe extern "C" fn stopping() {
    if let Some(callback) = STOP.get() {
        guard("server stopping", (), callback);
    }
}

impl Lifecycle {
    /// Register one callback per mod. The host waits for it before saving the worlds.
    pub fn on_stopping(
        self,
        registry: &mut Registry<'_>,
        callback: fn(),
    ) -> Result<(), RegistrationError> {
        STOP.set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the registration is live and the module stays loaded.
        RegistrationError::from_code(unsafe { (self.0.on_stopping)(registry.reg, stopping) })
    }
}
