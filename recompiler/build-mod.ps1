# Fabric mod JAR -> Pumpkin plugin: TeaVM (jar + fabric-shim) -> C -> wasm object -> linked
# into the fabric-host plugin component. Output: <OutDir>\<mod id>.wasm
param(
    [Parameter(Mandatory)] [string] $Jar,
    [string] $OutDir = (Join-Path (Split-Path $PSScriptRoot) 'target\fabric-mods')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$cache = Join-Path $root 'cache'
$env:JAVA_HOME = Join-Path $cache 'jdk-25.0.5+8'
$env:PATH = "$cache\gradle-9.9.1\bin;$env:JAVA_HOME\bin;$env:USERPROFILE\.cargo\bin;$env:PATH"
$Jar = (Resolve-Path $Jar).Path
$shim = Join-Path $PSScriptRoot 'fabric-shim'

Push-Location $shim
try {
    gradle generateC "-PmodJar=$Jar" --console=plain -q
    if ($LASTEXITCODE) { throw "TeaVM failed ($LASTEXITCODE)" }
} finally { Pop-Location }
$id = (Get-Content (Join-Path $shim 'build\mod-id.txt') -Raw).Trim()

$obj = Join-Path $shim "build\$id.o"
& (Join-Path $PSScriptRoot 'teavm-c-to-wasm.ps1') -GeneratedC (Join-Path $shim 'build\generated\teavm\c') `
    -Include (Join-Path $shim 'src\c\imports.h') -Out $obj

$env:MODDEDMC_TEAVM_OBJ = $obj
$env:MODDEDMC_MOD_ID = $id
$env:MODDEDMC_WASI_LIBS = Join-Path $cache 'wasi-sdk-34.0-x86_64-windows\share\wasi-sysroot\lib\wasm32-wasip2'
# A separate target dir per mod keeps cargo from reusing another mod's link.
$target = Join-Path $root "target\fabric-host\$id"
cargo build --manifest-path (Join-Path $root 'Cargo.toml') -p fabric-host --release --target wasm32-wasip2 --target-dir $target
if ($LASTEXITCODE) { throw "cargo failed ($LASTEXITCODE)" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$out = Join-Path $OutDir "$id.wasm"
Copy-Item (Join-Path $target 'wasm32-wasip2\release\fabric_host.wasm') $out -Force
Write-Host "built $out ($((Get-Item $out).Length) bytes)"
