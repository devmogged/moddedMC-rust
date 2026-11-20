//! ABI fixture consuming a service from a dependency loaded earlier.

use moddedmc_api::{Command, InterfaceUnavailable, Mod, RegistrationError, Registry, services};
use std::sync::OnceLock;

#[repr(C)]
struct EchoV1 {
    size: usize,
    increment: extern "C" fn(u32) -> u32,
}

static ECHO: OnceLock<&'static EchoV1> = OnceLock::new();

struct Consumer;

impl Mod for Consumer {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        // SAFETY: provider and consumer agree on the published repr(C) version 1 table.
        // SAFETY: the call reads a size-checked table only when the version is present.
        assert!(unsafe { services::require::<EchoV1>("abi_provider:echo", 2) }.is_err());
        let table = unsafe { services::require::<EchoV1>("abi_provider:echo", 1)? };
        let _ = ECHO.set(table);
        moddedmc_api::info("abi_consumer resolved echo v1 during startup");
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("abi_echo", "Check cross-mod service", 0)
    }

    fn command(&self, _tag: u64, _command: Command<'_>) -> i32 {
        if let Some(table) = ECHO.get() {
            let value = (table.increment)(35);
            moddedmc_api::info(&format!("abi_consumer service result={value}"));
            return 1;
        }
        0
    }
}

moddedmc_api::export_mod!(Consumer, id = "abi_consumer");
