# Rust mod ports

Every port ships as a `.mmc` in the server's `mods/` folder. Add or remove the file and restart. A pack contains the same stock server binary for its target, plus its own set of mod files.

## Build a port

Start with [Chunky](../mods/chunky), [Counter](../mods/counter) or [Wayshrine](../mods/wayshrine). Put the crate in `mods/<id>` and depend on [the SDK](../api), whose C tables are defined in [MOD-ABI.md](MOD-ABI.md).

1. Require the named interface versions the port uses in `Mod::start`. A server that lacks a required version logs its name and skips the mod before worlds load.
2. Keep mechanics in the mod. Create owns its kinetic networks, and each boss or dragon mod owns its AI. The host exposes operations the engine can perform safely. Plan new operations in [INTERFACES-PLAN.md](INTERFACES-PLAN.md).
3. Build with `scripts/pack-mod.ps1 -Crate <name>` or put the crate's path in a [pack manifest](PACKS.md). A pack can also use a prebuilt `.mmc`.

A new mechanic may need a server release that adds a generic engine hook and a named interface. Existing mods use the versions they were built against. This model gives up direct engine access and cross-mod compiler optimization.

The server's `pack` profile is optimized and incremental for local work. Use `release` with fat LTO for deployment. Each mod builds independently of the server.

## Measure against the original

Use the Java mod with the same seed and settings. Record throughput, MSPT and peak RSS where they apply. Ports should aim for several times the throughput and a fraction of the RAM, with outcomes checked against the original mod. Run `scripts/pregen-bench.ps1 -Radius 1000 -Runs 3` for Chunky. Results are in [BACKLOG.md](BACKLOG.md).
