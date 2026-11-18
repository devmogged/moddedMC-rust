# Freezes the M3 corpus: the top N Fabric 26.3 mods on Modrinth by downloads that can run on a
# server (server_side required or optional; client-only mods can never apply to us). Writes
# corpus/top500.tsv (committed) and downloads each pinned file to cache\corpus, checking sha512.
#   recompiler\corpus\freeze.ps1            # query + download
#   recompiler\corpus\freeze.ps1 -Download  # download only, from the committed list
param([int]$Count = 500, [switch]$Download)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot)
$list = Join-Path $PSScriptRoot 'top500.tsv'
$jars = Join-Path $root 'cache\corpus'
$headers = @{ 'User-Agent' = 'moddedMC-rust-corpus/0.1' }
$api = 'https://api.modrinth.com/v2'

function Get-Api($path) {
    for ($try = 0; ; $try++) {
        try { return Invoke-RestMethod -Headers $headers "$api$path" }
        catch {
            if ($try -ge 4) { throw }
            Start-Sleep -Seconds (2 * ($try + 1))  # rate limit (300/min) or a transient error
        }
    }
}

if (-not $Download) {
    $facets = [uri]::EscapeDataString('[["project_type:mod"],["categories:fabric"],["versions:26.3"],["server_side:required","server_side:optional"]]')
    $rows = [Collections.Generic.List[string]]::new()
    $rows.Add("rank`tslug`tproject_id`tversion_id`tversion`tfile`tsha512`tsize`tdownloads`tclient_side`tserver_side`turl")
    $offset = 0
    while ($rows.Count - 1 -lt $Count) {
        $page = Get-Api "/search?facets=$facets&index=downloads&limit=100&offset=$offset"
        if (-not $page.hits) { break }
        foreach ($hit in $page.hits) {
            if ($rows.Count - 1 -ge $Count) { break }
            $loaders = [uri]::EscapeDataString('["fabric"]'); $games = [uri]::EscapeDataString('["26.3"]')
            $versions = @(Get-Api "/project/$($hit.project_id)/version?loaders=$loaders&game_versions=$games")
            # Newest first; prefer a release, else whatever exists for 26.3.
            $v = ($versions | Where-Object version_type -eq 'release' | Select-Object -First 1)
            if (-not $v) { $v = $versions | Select-Object -First 1 }
            if (-not $v) { continue }
            $file = $v.files | Where-Object primary | Select-Object -First 1
            if (-not $file) { $file = $v.files | Select-Object -First 1 }
            $rank = $rows.Count
            $rows.Add(($rank, $hit.slug, $hit.project_id, $v.id, $v.version_number, $file.filename,
                    $file.hashes.sha512, $file.size, $hit.downloads, $hit.client_side, $hit.server_side, $file.url) -join "`t")
            Start-Sleep -Milliseconds 150
        }
        $offset += 100
        Write-Host "listed $($rows.Count - 1)"
    }
    [IO.File]::WriteAllLines($list, $rows, [Text.UTF8Encoding]::new($false))
}

New-Item -ItemType Directory -Force $jars | Out-Null
$entries = Import-Csv $list -Delimiter "`t"
foreach ($e in $entries) {
    $path = Join-Path $jars $e.file
    # Literal paths: some file names contain [ ], which PowerShell treats as wildcards.
    if (-not (Test-Path -LiteralPath $path) -or (Get-FileHash -LiteralPath $path -Algorithm SHA512).Hash -ne $e.sha512) {
        $client = [Net.WebClient]::new()
        $client.Headers.Add('User-Agent', $headers['User-Agent'])
        $client.DownloadFile($e.url, $path)
    }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA512).Hash -ne $e.sha512) { throw "sha512 mismatch: $($e.file)" }
}
Write-Host "corpus: $($entries.Count) jars in $jars"
