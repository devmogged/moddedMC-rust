# moddedMC-rust

**Exploring high-performance modded Minecraft in Rust.**

`moddedMC-rust` is an experimental project from [devmogged](https://github.com/devmogged) focused on bringing modded Minecraft gameplay to a Rust-based server environment. The idea is to explore native Rust implementations of features from Minecraft mods while taking advantage of the performance and efficiency of Rust.

It builds on [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), an open-source Minecraft server written in Rust, through our fork [devmogged/Pumpkin](https://github.com/devmogged/Pumpkin). Mods run entirely on the server, so players join with an unmodified Minecraft Java 26.3 client. Installing one works like on any modded server: drop the file into `mods/` and restart.

```
myserver/
  mods/
    wayshrine.mmc     a Rust mod (native code, one file for every platform)
    sethome.jar       an unchanged Fabric mod, converted to native code on first start
```

Rust mods talk to the server through a small, stable C ABI, so a mod built once keeps working when the server is updated, and it is about 100–200 KB. See [docs/MOD-ABI.md](docs/MOD-ABI.md) for server owners and mod authors. Big ports that need the whole engine (Create, bosses, dragons) are compiled into a pack server instead; see [docs/PORTS.md](docs/PORTS.md). The plan for the whole project is in [docs/ROADMAP.md](docs/ROADMAP.md), and the task list is in [docs/BACKLOG.md](docs/BACKLOG.md).

## Why Rust?

Rust offers memory safety without a garbage collector, fine-grained control over resource use, and strong support for concurrent systems. We're interested in what those properties could mean for the performance of heavily modded Minecraft servers.

## Current status

**M1 done: one custom block, end to end.** A content mod (`mods/counter`) registers `demo:counter` through the mod API. Players place it, it ticks, they use it, and it keeps its identity and state across chunk reloads and restarts. 10,000 of them add about 1–1.5 ms to the median tick. Details and numbers: [docs/STATUS.md](docs/STATUS.md). Design: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

**M2 done: a playable hand port.** `mods/wayshrine` adds discoverable fast-travel shrines with a Java dialog menu, persistence and break handling, tested end to end with a headless bot.

**M3 done: an unchanged Fabric JAR runs on Pumpkin.** Drop the `sethome` fixture JAR into `mods/`: on first start the server converts it (TeaVM to C to a native mod) and it passes the same scenario on Pumpkin as on a real Fabric 26.3 server. See [recompiler/README.md](recompiler/README.md). The frozen top-500 Modrinth corpus shows the limit: 83% of mods with code use Mixin, which the shim cannot run.

**M4: the host app.** `moddedmc run` starts a server folder, forwards its console, and opens a tunnel so friends can join:

```powershell
cargo build -p moddedmc --release
target\release\moddedmc.exe run --dir myserver --server engine\target\pack\pack-dev.exe --tunnel playit
# [moddedmc] players join <name>.tun.ply.gg (or <ip>:<port>)
```

`--tunnel playit` uses a linked playit agent (run `playit` once to link it) and the account's Minecraft Java tunnel to `127.0.0.1:25565`. `--tunnel tailscale` prints this machine's tailnet address. Mods go in `<dir>\mods`. A new folder gets telemetry off.

### Build and run (Windows, PowerShell)

```powershell
git submodule update --init
# Pumpkin's build reads vanilla data from Mojang's 26.3 server jar.
$env:PUMPKIN_MINECRAFT_SERVER_JAR = "<path to the 26.3 server.jar>"
scripts\engine.ps1 build                                     # engine\target\pack\pack-dev.exe + test bot (first build ~12 min, then seconds)
scripts\pack-mod.ps1 -Crate counter                          # build a mod -> target\mods\counter.mmc
scripts\scenario.ps1 -Scenario tests\scenarios\m1-counter.txt -RunDir run\m1 -Fresh
```

`scripts\engine.ps1 build` also builds the headless test client the scenarios use (`engine\target\pack\moddedmc-bot.exe`).

Existing Fabric, Forge, or NeoForge mods **do not automatically run** on a Rust Minecraft server. Bringing their functionality over may require substantial reimplementation, and some modded experiences may also need client-side changes. Comparisons against Fabric servers have not been benchmarked yet.

## Collaboration

Bug reports, bug fixes and new mods are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first: features and changes to the mod ABI or the engine patch start as an issue.

## Acknowledgements

- [Pumpkin MC](https://pumpkinmc.org/) — the Rust Minecraft server project that inspired this idea.

This is an independent community project, not affiliated with or endorsed by Mojang Studios, Microsoft, or the Pumpkin project. Minecraft is a trademark of Mojang Studios.
