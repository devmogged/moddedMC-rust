# Fabric mod JAR -> moddedMC mod: TeaVM (jar + fabric-shim) -> C -> clang object -> linked into the
# fabric-host library, which speaks the stable mod ABI. The server runs this itself the first time
# it sees a .jar in mods/ (result cached in mods/.cache); run it by hand to convert ahead of time.
#   recompiler\build-mod.ps1 -Jar sethome.jar [-OutFile mods\.cache\sethome.dll | -OutDir target\fabric-mods]
param(
    [Parameter(Mandatory)] [string] $Jar,
    [string] $OutFile, # exact output path; wins over OutDir
    [string] $OutDir   # default: target\fabric-mods, as <mod id>.dll
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $root 'target\fabric-mods' }
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

# fabric-host owns the entry point and calls teavm_main once, so TeaVM's main is renamed.
$obj = Join-Path $shim "build\$id.o"
clang -O2 -w -include (Join-Path $shim 'src\c\imports.h') '-Dmain=teavm_main' `
    -c (Join-Path $shim 'build\generated\teavm\c\all.c') -o $obj
if ($LASTEXITCODE) { throw "clang failed ($LASTEXITCODE)" }

$env:MODDEDMC_TEAVM_OBJ = $obj
$env:MODDEDMC_MOD_ID = $id
Push-Location $root
try {
    cargo build --release -p fabric-host
    if ($LASTEXITCODE) { throw "cargo failed ($LASTEXITCODE)" }
} finally { Pop-Location }
if (-not $OutFile) { $OutFile = Join-Path $OutDir "$id.dll" }
New-Item -ItemType Directory -Force (Split-Path $OutFile) | Out-Null
Copy-Item (Join-Path $root 'target\release\fabric_host.dll') $OutFile -Force
Write-Host "built $OutFile ($((Get-Item $OutFile).Length) bytes)"
