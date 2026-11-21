# Simple Storage Network

Partial port of Simple Storage Network NeoForge 26.1.2-1.13.2. Network Cables connect to other Network Cables on all six faces. Each arm has matching display and collision geometry. Connections update when a cable is placed, removed, or loaded. Models use a vanilla texture.

Build with `scripts/pack-mod.ps1 -Crate simple-storage-network`. With the engine and bot available, run the `simple-storage-network-cable.txt` and `simple-storage-network-connections.txt` scenarios under `tests/scenarios/` on separate ports. Operators can inspect a loaded cable's state with `/storage_network_cable <x> <y> <z>`.

Waterlogging, connections to inventory blocks, item transfer, terminals, and comparison with the original Java mod remain open.
