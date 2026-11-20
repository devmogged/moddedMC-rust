//! ABI fixture publishing a process-lifetime service from a native mod.

use moddedmc_api::{InterfaceUnavailable, Mod, RegistrationError, Registry, services};
use std::{ffi::c_void, sync::OnceLock};

#[repr(C)]
struct EchoV1 {
    size: usize,
    increment: extern "C" fn(u32) -> u32,
}

extern "C" fn increment(n: u32) -> u32 {
    n + 7
}

static ECHO: EchoV1 = EchoV1 {
    size: size_of::<EchoV1>(),
    increment,
};

struct Provider;
static SERVICES: OnceLock<services::Services> = OnceLock::new();

impl Mod for Provider {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = SERVICES.set(services::require_host()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let api = *SERVICES
            .get()
            .expect("service interface required during startup");
        // SAFETY: ECHO is repr(C), begins with size and is static for the process lifetime.
        unsafe {
            api.publish(
                registry,
                "abi_provider:echo",
                1,
                (&raw const ECHO).cast::<c_void>(),
            )?
        };
        // SAFETY: the same static table is valid for the lifetime of this process.
        assert!(
            unsafe {
                api.publish(
                    registry,
                    "abi_provider:echo",
                    1,
                    (&raw const ECHO).cast::<c_void>(),
                )
            }
            .is_err()
        );
        // SAFETY: the same static table is valid, but a foreign namespace is forbidden.
        assert!(
            unsafe {
                api.publish(
                    registry,
                    "other:echo",
                    1,
                    (&raw const ECHO).cast::<c_void>(),
                )
            }
            .is_err()
        );
        moddedmc_api::info("abi_provider published echo v1");
        Ok(())
    }
}

moddedmc_api::export_mod!(Provider, id = "abi_provider");
