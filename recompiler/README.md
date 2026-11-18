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

## Next

- Run the fixture on a Fabric 26.3 server and record the bot transcript (oracle).
- Shim library: Java implementations of the Minecraft/Fabric/Brigadier API the fixture
  uses, calling `@Import` functions that the glue maps onto `pumpkin:plugin` WIT.
- Exports: TeaVM drops code `main` can't reach, so entry points need a reachable root.
