# Create: New Age

Partial source-backed port of **Create: New Age `1.2.0+mc1.21.1`** (NeoForge). Original code is BSD-3-Clause (Antarctic Gardens). No original game assets are bundled.

The Rust crate currently implements the generator coil's twelve magnet offsets around X, Y and Z shafts, its magnet-dependent stress and FE production, and the consume-once generated-energy buffer. Source: `GeneratorCoilBlockEntity` and `RelativeBlockPos`, checked against the published 1.2.0 NeoForge jar and the public 1.21.1 branch. Conversion uses the original configurable `suToEnergy` value, supplied as a parameter.

Tests: `cargo test -p create-new-age`, `cargo clippy -p create-new-age --all-targets -- -D warnings`.

This is an `rlib` for source-comparison and standalone logic tests, **not yet a loadable `.mmc`**. Full gameplay needs Create's inter-mod kinetic source/consumer service (`WS-4-01`) and registered electric network, magnets, item/block content and persistence. An original Java-mod oracle must verify stress, power delivery and restart before calling the mechanic ported.
