# Recompiler (M3, in progress)

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

## Next

- Shim library: Java implementations of the Minecraft/Fabric/Brigadier API the fixture
  uses, calling `@Import` functions that the glue maps onto `pumpkin:plugin` WIT.
- Exports: TeaVM drops code `main` can't reach, so entry points need a reachable root.
