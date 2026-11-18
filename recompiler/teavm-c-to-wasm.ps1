# Compiles TeaVM's generated C (all.c) plus host glue into a wasm32-wasip2 component.
# TeaVM targets Unix; on WASI we take its Emscripten branches (malloc heap, no
# signal-driven fibers: a mod must never block) and wasi-libc's POSIX emulations.
# setjmp/longjmp (TeaVM's exceptions) use the Wasm exception-handling proposal,
# which Pumpkin's Wasmtime enables.
param(
    [Parameter(Mandatory)] [string] $GeneratedC,   # TeaVM output dir containing all.c
    [string[]] $Glue = @(),                        # extra C files; none = emit only the object at $Out
    [Parameter(Mandatory)] [string] $Out,
    [string[]] $Include = @()                      # header force-included before all.c (declarations of imports)
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$cc = Join-Path $root 'cache\wasi-sdk-34.0-x86_64-windows\bin\clang.exe'
$shim = Join-Path $PSScriptRoot 'wasi-shim'
$flags = @(
    '--target=wasm32-wasip2', '-O2', '-w', '-Wno-error=return-mismatch',
    '-I', (Join-Path $shim 'include'), '-include', (Join-Path $shim 'pre.h'),
    '-D__EMSCRIPTEN__', '-D_WASI_EMULATED_MMAN', '-D_WASI_EMULATED_SIGNAL',
    '-D_WASI_EMULATED_PROCESS_CLOCKS', '-D_WASI_EMULATED_GETPID',
    '-mllvm', '-wasm-enable-sjlj', '-mllvm', '-wasm-use-legacy-eh=false'
)
foreach ($h in $Include) { $flags += @('-include', $h) }
# The glue owns the entry point: it calls teavm_main once to bring the Java runtime up,
# so only TeaVM's translation unit gets the rename.
$obj = if ($Glue) { "$Out.teavm.o" } else { $Out }
& $cc @flags '-Dmain=teavm_main' -c (Join-Path $GeneratedC 'all.c') -o $obj
if ($LASTEXITCODE) { throw "clang failed on all.c ($LASTEXITCODE)" }
if (-not $Glue) { return }  # object only: fabric-host links it
$libs = '-lsetjmp', '-lwasi-emulated-mman', '-lwasi-emulated-signal', '-lwasi-emulated-process-clocks', '-lwasi-emulated-getpid'
& $cc @flags $obj @Glue @libs -o $Out
if ($LASTEXITCODE) { throw "clang failed ($LASTEXITCODE)" }
