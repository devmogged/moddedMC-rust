# Manages our patch stack on the pinned Pumpkin submodule.
#   scripts\engine.ps1 apply   reset engine/pumpkin to the pinned commit and apply patches\pumpkin.patch
#   scripts\engine.ps1 save    write the current engine/pumpkin changes to patches\pumpkin.patch
#   scripts\engine.ps1 build   apply, then cargo build --release the server and the native mods
# The submodule pointer always stays at the upstream commit; our changes live only in the patch.
param([Parameter(Mandatory)][ValidateSet('apply', 'save', 'build')] [string]$Action)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$engine = Join-Path $root 'engine\pumpkin'
$patch = Join-Path $root 'patches\pumpkin.patch'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

function Invoke-Git { git -c core.safecrlf=false -C $engine @args; if ($LASTEXITCODE) { throw "git $args failed" } }

switch ($Action) {
    'save' {
        # Intent-to-add makes new files show up in the diff.
        Invoke-Git add -N .
        $diff = git -c core.safecrlf=false -C $engine diff --binary
        New-Item -ItemType Directory -Force (Split-Path $patch) | Out-Null
        [IO.File]::WriteAllText($patch, (($diff -join "`n") + "`n"))
        Invoke-Git reset -q
        "saved $((Get-Item $patch).Length) bytes to patches\pumpkin.patch"
    }
    { $_ -in 'apply', 'build' } {
        Invoke-Git reset -q --hard
        Invoke-Git clean -fdq
        Invoke-Git apply --whitespace=nowarn $patch
        "applied patches\pumpkin.patch on $(git -C $engine rev-parse --short HEAD)"
        if ($Action -eq 'build') {
            if (-not $env:PUMPKIN_MINECRAFT_SERVER_JAR) {
                $env:PUMPKIN_MINECRAFT_SERVER_JAR = Join-Path $root 'cache\mc-26.3\server.jar'
            }
            Push-Location $engine
            # Mods are .dll plugins and must come from the same build as the server.
            try { cargo build --release -p pumpkin -p counter -p wayshrine; if ($LASTEXITCODE) { throw 'build failed' } }
            finally { Pop-Location }
        }
    }
}
