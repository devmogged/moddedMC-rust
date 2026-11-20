//! Create Crafts & Additions, release neoforge-1.21.1-1.7.2, MIT.
//! Energy rates and tick transitions checked against this release's Java bytecode.
//! Full kinetic source/consumer registration waits on WS-0 create:kinetics v1.

mod content;
pub mod electricity;
pub mod energy_storage;

moddedmc_api::export_mod!(content::CreateAddition);
