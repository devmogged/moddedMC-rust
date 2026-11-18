# Recompiler (M3)

Goal: take an unchanged Fabric 26.3 mod JAR and produce a native moddedMC-rust mod (`.dll`), AOT.

## Pipeline

1. TeaVM 0.16 C backend (`generateC`) turns JVM bytecode into C with its own GC.
   A Java `@Import(module, name)` becomes a plain C call to `name`.
2. The system clang (LLVM 22) compiles that C to a native object (`-Dmain=teavm_main`).
3. `fabric-host` links the object into a native Pumpkin plugin and implements the imports.

Until 2026-11-18 this produced a `wasm32-wasip2` component through wasi-sdk; the project
moved to native plugins for speed, and the WASM path was removed.

Toolchain (all in the ignored `cache/` except LLVM): Temurin JDK 25, Gradle 9.9.1, LLVM clang.

## Fixture and oracle

`fixtures/sethome` is an ordinary Fabric 26.3 mod (no mixins): `/sethome`, `/home`
with a 10 s cooldown, homes saved to `config/sethome-homes.txt`. Its built JAR is the
recompiler input; a real Fabric server running the same JAR is the oracle.

## Oracle (done)

`scripts/fabric-oracle.ps1` prepares a real Fabric 26.3 server (loader 0.20.1, Fabric API
0.166.2+26.3) with the fixture; `tests/oracle/sethome.txt` drives it with `moddedmc-bot` and
passes: no home, set, teleport home, cooldown (8 s left after 2 s), and the home survives a
restart. The same scenario is the acceptance test for the recompiled mod on Pumpkin.

Making the bot pass a vanilla server needed three fixes Pumpkin never enforced: answer the
configuration ping (Fabric waits for it), send `particle_status` in client information
(1.21.2+, Pumpkin patch), and send client tick-end after each move (26.3 allows one position
per client tick and kicks otherwise).

## Fabric JAR on Pumpkin (done for the fixture)

    scripts\engine.ps1 build      # the plugin must come from the server's own build
    recompiler\build-mod.ps1 -Jar recompiler\fixtures\sethome\build\libs\sethome-1.0.0.jar
    # -> target\fabric-mods\sethome.dll; copy into plugins\ and run tests\oracle\sethome.txt

The unchanged `sethome-1.0.0.jar` passes the same oracle scenario on Pumpkin as on Fabric.

- `fabric-shim/` is compiled by TeaVM together with the mod JAR and real Brigadier 1.3.11
  (MIT, from Mojang's Maven). It holds the Minecraft/Fabric/authlib classes the mod links
  against, under their real names and descriptors, implemented over `@Import` host calls
  (`moddedmc.rt.Host`). `ModRuntime.main` runs the `fabric.mod.json` main entrypoints
  (generated as plain constructor calls; TeaVM has no reflection), fires
  `CommandRegistrationCallback` and registers each root literal with Pumpkin.
  `@Export mmc_run_command` runs a command through the mod's Brigadier dispatcher.
- `fabric-host/` is a native Pumpkin plugin in the server's workspace. It links the TeaVM
  object and implements the `mmc_*` imports directly on Pumpkin's types. TeaVM's runtime is
  single-threaded, so every call into Java holds one lock.
- TeaVM's `main` is renamed `teavm_main` and called once from `on_load`; exports are then
  callable any number of times.

Found on the way:

- TeaVM 0.16 `Files.createDirectories` on a one-level path calls `createDirectory`, which
  stats the parent and throws if the directory already exists (the JDK does not). Fabric's
  `config/` is created by the host before the mod runs, as Fabric always has one.
- Pumpkin sends the teleport before the chat line, as vanilla does; vanilla then resyncs the
  position once more. The oracle spec checks teleport-then-chat on both servers.

Known shortcuts (each marked `shortcut:` in code): root literals only (no arguments reach
Brigadier yet), plain-text components, absolute teleports, player handles valid for one
command.

## Corpus (frozen)

`corpus/top500.tsv` pins the top 500 Fabric 26.3 mods on Modrinth by downloads that can run on
a server (Modrinth `server_side` required or optional): version id, file and sha512.
`corpus/freeze.ps1 -Download` fetches them into `cache/corpus` (1.2 GB). `corpus/report.ps1`
runs `Analyze.java`, which resolves every class and member reference in each jar, nested jars
included, against the shim, Brigadier and TeaVM's class library. Results: `corpus/report.md`
(summary) and `corpus/report.tsv` (per mod).

What it says (2026-11-18):

- 45 of 500 contain no Java code: datapacks and resource packs shipped as mods. They need
  datapack support in the engine, not the recompiler.
- 378 of the 455 mods with code (83%) use Mixin. A Java shim cannot run those; they patch
  Minecraft's own bytecode at injection points.
- 60 more have no Mixin but call into another mod (a library such as Cloth Config), so they need
  that mod recompiled alongside them.
- 17 are self-contained Mixin-free mods, the shim's direct target. 5 are covered today, but
  those are one-class stubs. Covering all 17 takes 582 more members, mostly registries, blocks,
  items and creative tabs.

So the shim alone reaches about 4% of mods with code. Most of the corpus depends on
supporting Mixin. That means either running Mixin against a recompiled Minecraft, or mapping
common injection points onto engine hooks. That decision comes before growing the shim.
