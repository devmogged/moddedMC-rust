//! Create: New Age 1.2.0+mc1.21.1 (NeoForge).
//! Passive items are registered in the native .mmc.
//! Generator-coil, wiring and energy mechanics still await host integration.

mod content;
pub mod electrical_network;
pub mod energy_storage;
pub mod generator_coil;

moddedmc_api::export_mod!(content::CreateNewAge);
