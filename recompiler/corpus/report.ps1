# Regenerates recompiler/corpus/report.md and report.tsv from the frozen corpus (freeze.ps1 -Download first).
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot)
$cache = Join-Path $root 'cache'
$env:JAVA_HOME = Join-Path $cache 'jdk-25.0.5+8'
$env:PATH = "$cache\gradle-9.9.1\bin;$env:JAVA_HOME\bin;$env:PATH"
$shim = Join-Path $root 'recompiler\fabric-shim'
# The shim build needs some mod jar; the fixture is enough to compile the shim classes.
$fixture = Join-Path $root 'recompiler\fixtures\sethome\build\libs\sethome-1.0.0.jar'
Push-Location $shim
try {
    gradle classes "-PmodJar=$fixture" --console=plain -q
    if ($LASTEXITCODE) { throw "shim build failed ($LASTEXITCODE)" }
} finally { Pop-Location }
$gradleCache = Join-Path $env:USERPROFILE '.gradle\caches\modules-2\files-2.1'
$brigadier = Get-ChildItem (Join-Path $gradleCache 'com.mojang\brigadier\1.3.11') -Recurse -Filter 'brigadier-1.3.11.jar' | Select-Object -First 1
$classlib = Get-ChildItem (Join-Path $gradleCache 'org.teavm\teavm-classlib\0.16.0') -Recurse -Filter 'teavm-classlib-0.16.0.jar' | Select-Object -First 1
java (Join-Path $PSScriptRoot 'Analyze.java') (Join-Path $PSScriptRoot 'top500.tsv') (Join-Path $cache 'corpus') $PSScriptRoot `
    (Join-Path $shim 'build\classes\java\main') $brigadier.FullName $classlib.FullName
if ($LASTEXITCODE) { throw "analyzer failed ($LASTEXITCODE)" }
