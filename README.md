# moddedMC-rust

**Exploring high-performance modded Minecraft in Rust.**

`moddedMC-rust` is an experimental project from [devmogged](https://github.com/devmogged) focused on bringing modded Minecraft gameplay to a Rust-based server environment. The idea is to explore native Rust implementations of features from Minecraft mods while taking advantage of the performance and efficiency of Rust.

It builds on [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), an open-source Minecraft server written in Rust, pinned to one upstream commit plus a small patch. Mods are native Rust plugins (`.dll`/`.so`) that run entirely on the server, so players join with an unmodified Minecraft Java 26.3 client.

## Why Rust?

Rust offers memory safety without a garbage collector, fine-grained control over resource use, and strong support for concurrent systems. We're interested in what those properties could mean for the performance of heavily modded Minecraft servers.

## Current status

**M1 done: one custom block, end to end.** A content mod (`mods/counter`) registers `demo:counter` through the `moddedmc:mods` WIT API. Players place it, it ticks, they use it, and it keeps its identity and state across chunk reloads and restarts. 10,000 of them add about 1–1.5 ms to the median tick. Details and numbers: [docs/STATUS.md](docs/STATUS.md). Design: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

**M2 done: a playable hand port.** `mods/wayshrine` adds discoverable fast-travel shrines with a Java dialog menu, persistence and break handling, tested end to end with a headless bot.

**M3 done: an unchanged Fabric JAR runs on Pumpkin.** The `sethome` fixture JAR goes through TeaVM to C, then to a native plugin, and passes the same scenario on Pumpkin as on a real Fabric 26.3 server. See [recompiler/README.md](recompiler/README.md). The frozen top-500 Modrinth corpus shows the limit: 83% of mods with code use Mixin, which the shim cannot run.

**M4: the host app.** `moddedmc run` starts a server folder, forwards its console, and opens a tunnel so friends can join:

```powershell
cargo build -p moddedmc --release
target\release\moddedmc.exe run --dir myserver --server engine\pumpkin\target\release\pumpkin.exe --tunnel playit
# [moddedmc] players join <name>.tun.ply.gg (or <ip>:<port>)
```

`--tunnel playit` uses a linked playit agent (run `playit` once to link it) and the account's Minecraft Java tunnel to `127.0.0.1:25565`. `--tunnel tailscale` prints this machine's tailnet address. Mods go in `<dir>\plugins`. A new folder gets telemetry off.

### Build and run (Windows, PowerShell)

```powershell
git submodule update --init
# Pumpkin's build reads vanilla data from Mojang's 26.3 server jar.
$env:PUMPKIN_MINECRAFT_SERVER_JAR = "<path to the 26.3 server.jar>"
scripts\engine.ps1 build                                     # apply patches\pumpkin.patch, build the server and mods
scripts\scenario.ps1 -Scenario tests\scenarios\m1-counter.txt -RunDir run\m1 -Fresh
```

The scenario test needs the headless test client: `cargo build -p moddedmc-bot` in `engine\pumpkin`.

Existing Fabric, Forge, or NeoForge mods **do not automatically run** on a Rust Minecraft server. Bringing their functionality over may require substantial reimplementation, and some modded experiences may also need client-side changes. Comparisons against Fabric servers have not been benchmarked yet.

## Collaboration

Discussion, experimentation, and contributions are welcome. The details will be decided together as the project develops.

## Acknowledgements

- [Pumpkin MC](https://pumpkinmc.org/) — the Rust Minecraft server project that inspired this idea.

This is an independent community project, not affiliated with or endorsed by Mojang Studios, Microsoft, or the Pumpkin project. Minecraft is a trademark of Mojang Studios.
