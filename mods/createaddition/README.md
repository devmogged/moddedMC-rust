# Create Crafts & Additions

This is the initial Rust logic port for **Create Crafts & Additions 1.7.2** (NeoForge 1.21.1), using the release jar as the behavioral reference. The original mod is licensed under MIT by MRH0.

`src/electricity.rs` implements the electric motor's energy-rate and active/inactive tick transitions and the alternator's FE generation and stress calculations. `src/energy_storage.rs` ports `InternalEnergyStorage`, including capped FE input/output, uncapped internal consumption/production and its exact `energy` NBT key. The original Java methods were checked in the published 1.7.2 binary using `javap -c -p` and against the corresponding public source tree. Config values remain parameters so that the eventual service uses the real values from the runtime config.

The current crate is an `rlib` for focused Rust logic tests. **It is not a loadable `.mmc` or a working energy network.** To implement that, WS-0 must publish the `create:kinetics` inter-mod service (request `N:/mmc-coord/requests/WS-4-01.md`) and provide the generic block content and FE interface required by the machines. At that point the crate will become a standalone `cdylib` and get an original-mod scenario covering live RPM, energy, stress, NBT and restart.

Run: `cargo test -p createaddition` and `cargo clippy -p createaddition --all-targets -- -D warnings`.
