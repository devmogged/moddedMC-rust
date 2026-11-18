# Fabric mod JAR -> native Pumpkin plugin: TeaVM (jar + fabric-shim) -> C -> clang object ->
# linked into the fabric-host plugin. Output: <OutDir>\<mod id>.dll
# Builds in the server's workspace and release profile, so run scripts\engine.ps1 build first
# and rebuild mods whenever the server is rebuilt.
param(
    [Parameter(Mandatory)] [string] $Jar,
    [string] $OutDir   # default: target\fabric-mods
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $root 'target\fabric-mods' }
$cache = Join-Path $root 'cache'
$env:JAVA_HOME = Join-Path $cache 'jdk-25.0.5+8'
$env:PATH = "$cache\gradle-9.9.1\bin;$env:JAVA_HOME\bin;$env:USERPROFILE\.cargo\bin;$env:PATH"
if (-not $env:PUMPKIN_MINECRAFT_SERVER_JAR) { $env:PUMPKIN_MINECRAFT_SERVER_JAR = Join-Path $cache 'mc-26.3\server.jar' }
$Jar = (Resolve-Path $Jar).Path
$shim = Join-Path $PSScriptRoot 'fabric-shim'

Push-Location $shim
try {
    gradle generateC "-PmodJar=$Jar" --console=plain -q
    if ($LASTEXITCODE) { throw "TeaVM failed ($LASTEXITCODE)" }
} finally { Pop-Location }
$id = (Get-Content (Join-Path $shim 'build\mod-id.txt') -Raw).Trim()

# fabric-host owns the entry point and calls teavm_main once, so TeaVM's main is renamed.
$obj = Join-Path $shim "build\$id.o"
clang -O2 -w -include (Join-Path $shim 'src\c\imports.h') '-Dmain=teavm_main' `
    -c (Join-Path $shim 'build\generated\teavm\c\all.c') -o $obj
if ($LASTEXITCODE) { throw "clang failed ($LASTEXITCODE)" }

$env:MODDEDMC_TEAVM_OBJ = $obj
$env:MODDEDMC_MOD_ID = $id
$engine = Join-Path $root 'engine\pumpkin'
Push-Location $engine
try {
    cargo build --release -p fabric-host
    if ($LASTEXITCODE) { throw "cargo failed ($LASTEXITCODE)" }
} finally { Pop-Location }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$out = Join-Path $OutDir "$id.dll"
Copy-Item (Join-Path $engine 'target\release\fabric_host.dll') $out -Force
Write-Host "built $out ($((Get-Item $out).Length) bytes)"
