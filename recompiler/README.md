# Recompiler (M3: fixture done, corpus next)

Goal: take an unchanged Fabric 26.3 mod JAR and produce a moddedMC-rust WASM mod, AOT.

## Pipeline (proven by `probe/`)

1. TeaVM 0.16 C backend (`generateC`) turns JVM bytecode into C with its own GC.
   The WasmGC backend was rejected: its output imports ~25 JavaScript runtime
   functions (JS strings, JSO, exceptions) and GC types can't cross the component boundary.
2. `teavm-c-to-wasm.ps1` compiles that C with wasi-sdk 34 to a `wasm32-wasip2` component.
   A Java `@Import(module, name)` becomes a plain C call to `name`, satisfied by glue C.
3. The probe runs on Wasmtime 50 (`wasmtime run -W exceptions=y`), the version Pumpkin pins.

Toolchain (all in the ignored `cache/`): Temurin JDK 25, Gradle 9.9.1, wasi-sdk 34.

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

    recompiler\build-mod.ps1 -Jar recompiler\fixtures\sethome\build\libs\sethome-1.0.0.jar
    # -> target\fabric-mods\sethome.wasm; copy into plugins\ and run tests\oracle\sethome.txt

The unchanged `sethome-1.0.0.jar` passes the same oracle scenario on Pumpkin as on Fabric.

- `fabric-shim/` is compiled by TeaVM together with the mod JAR and real Brigadier 1.3.11
  (MIT, from Mojang's Maven). It holds the Minecraft/Fabric/authlib classes the mod links
  against, under their real names and descriptors, implemented over `@Import` host calls
  (`moddedmc.rt.Host`). `ModRuntime.main` runs the `fabric.mod.json` main entrypoints
  (generated as plain constructor calls; TeaVM has no reflection), fires
  `CommandRegistrationCallback` and registers each root literal with Pumpkin.
  `@Export mmc_run_command` runs a command through the mod's Brigadier dispatcher.
- `fabric-host/` is an ordinary Rust Pumpkin plugin (`pumpkin-plugin-api`) that links the
  TeaVM object and implements the `mmc_*` imports. That keeps all WIT work in Rust; C
  bindings for the whole plugin world would be ~600 KB of generated glue.
- TeaVM's `main` is renamed `teavm_main` and called once from `on_load`; exports are then
  callable any number of times (`probe/` checks this under plain Wasmtime).
- Output: 2.0 MB component. First server start 2.2 s (Wasmtime compiles it once), 171 ms
  with the module cache warm.

Found on the way:

- TeaVM 0.16 `Files.createDirectories` on a one-level path calls `createDirectory`, which
  stats the parent (`/` or `.`, invisible under WASI) and throws if the directory already
  exists (the JDK does not). Fabric's config dir is therefore `/data/config` (two levels,
  inside the plugin's `data` preopen), created at load as Fabric always has `config/`.
- Pumpkin sends the teleport before the chat line, as vanilla does; vanilla then resyncs the
  position once more. The oracle spec now checks teleport-then-chat on both servers.

Known shortcuts (each marked `shortcut:` in code): root literals only (no arguments reach
Brigadier yet), plain-text components, absolute teleports, player handles valid for one
command, `getTickCount` backed by overworld age.

## Next

- Corpus: freeze the top 500 Modrinth Fabric mods for 26.3 and report, per mod, which
  classes and members outside the JDK it references, against what the shim implements.
- Grow the shim from that report, most-referenced API first.
