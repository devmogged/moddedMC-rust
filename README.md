# moddedMC-rust

**Exploring high-performance modded Minecraft in Rust.**

`moddedMC-rust` is an experimental project from [devmogged](https://github.com/devmogged) focused on bringing modded Minecraft gameplay to a Rust-based server environment. The idea is to explore native Rust implementations of features from Minecraft mods while taking advantage of the performance and efficiency of Rust.

The project is inspired by [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), an open-source Minecraft server written in Rust. Pumpkin is a potential foundation, not an architecture decision or a dependency we've committed to.

## Why Rust?

Rust offers memory safety without a garbage collector, fine-grained control over resource use, and strong support for concurrent systems. We're interested in what those properties could mean for the performance of heavily modded Minecraft servers.

## Current status

**Early concept — no working implementation yet.** This repository is a starting point for experimentation and collaboration. No particular architecture, mod format, or compatibility layer has been selected.

Existing Fabric, Forge, or NeoForge mods **do not automatically run** on a Rust Minecraft server. Bringing their functionality over may require substantial reimplementation, and some modded experiences may also need client-side changes. Performance improvements are a goal to investigate, not a benchmarked claim.

## Collaboration

Discussion, experimentation, and contributions are welcome. The details will be decided together as the project develops.

## Acknowledgements

- [Pumpkin MC](https://pumpkinmc.org/) — the Rust Minecraft server project that inspired this idea.

This is an independent community project, not affiliated with or endorsed by Mojang Studios, Microsoft, or the Pumpkin project. Minecraft is a trademark of Mojang Studios.
