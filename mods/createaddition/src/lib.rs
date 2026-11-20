//! Create Crafts & Additions, release neoforge-1.21.1-1.7.2, MIT.
//! Energy and rolling-mill transitions are source-checked; gameplay wiring awaits
//! WS-0's Create kinetic service and generic energy capability.

mod content;
pub mod electricity;
pub mod energy_storage;
pub mod rolling_mill;

moddedmc_api::export_mod!(content::CreateAddition);
