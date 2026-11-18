# Prepares a run dir with a real Fabric 26.3 server (the recompiler's test oracle):
# Fabric launcher, Fabric API and the given mod JARs, offline and uncompressed so moddedmc-bot can join.
#   scripts\fabric-oracle.ps1 -RunDir run\oracle-sethome -Mods recompiler\fixtures\sethome\build\libs\sethome-1.0.0.jar
# Then: scripts\scenario.ps1 -RunDir run\oracle-sethome -Exe cache\jdk-25.0.5+8\bin\java.exe -ExeArgs '-jar fabric-server-launch.jar nogui' -Scenario ...
param(
    [Parameter(Mandatory)] [string]$RunDir,
    [string[]]$Mods = @(),
    [string]$Loader = '0.20.1',
    [string]$Installer = '1.1.2',
    [string]$FabricApi = '0.166.2+26.3'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$cache = Join-Path $root 'cache\fabric'
New-Item -ItemType Directory -Force $cache | Out-Null
$launcher = Join-Path $cache "fabric-server-26.3-$Loader-$Installer.jar"
if (-not (Test-Path $launcher)) {
    Invoke-WebRequest -UseBasicParsing "https://meta.fabricmc.net/v2/versions/loader/26.3/$Loader/$Installer/server/jar" -OutFile $launcher
}
$api = Join-Path $cache "fabric-api-$FabricApi.jar"
if (-not (Test-Path $api)) {
    Invoke-WebRequest -UseBasicParsing "https://maven.fabricmc.net/net/fabricmc/fabric-api/fabric-api/$FabricApi/fabric-api-$FabricApi.jar" -OutFile $api
}
$RunDir = Join-Path $root $RunDir
if (Test-Path $RunDir) { Get-ChildItem $RunDir | Remove-Item -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $RunDir 'mods') | Out-Null
Copy-Item $launcher (Join-Path $RunDir 'fabric-server-launch.jar')
Copy-Item $api (Join-Path $RunDir 'mods')
foreach ($m in $Mods) { Copy-Item (Join-Path $root $m) (Join-Path $RunDir 'mods') }
# The vanilla server jar is already cached for Pumpkin's build; reuse it instead of downloading again.
Copy-Item (Join-Path $root 'cache\mc-26.3\server.jar') (Join-Path $RunDir 'server.jar')
[IO.File]::WriteAllText((Join-Path $RunDir 'eula.txt'), "eula=true`n")
[IO.File]::WriteAllText((Join-Path $RunDir 'server.properties'), @"
online-mode=false
white-list=false
enforce-whitelist=false
network-compression-threshold=-1
view-distance=4
simulation-distance=4
spawn-protection=0
enable-command-block=false
level-type=minecraft\:normal
"@)
