//! Corpse, targeting henkelmax Corpse 1.1.22+26.3 (NeoForge).
//!
//! This is the policy-only first slice. No death event, item transfer,
//! custom corpse entity, or inventory GUI is installed yet; see
//! docs/ws/corpse.md and the coordination-board ABI requests.

pub mod rules;

use moddedmc_api::{Mod, RegistrationError, Registry};

struct Corpse;

impl Mod for Corpse {
    fn register(&self, _registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        // Register the death/corpse hooks when the host exposes them.
        Ok(())
    }
}

moddedmc_api::export_mod!(Corpse);
