param(
    [Parameter(Mandatory = $true)][string]$BackupPath,
    [Parameter(Mandatory = $true)][string]$ManifestPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$backup = [IO.Path]::GetFullPath($BackupPath)
$manifest = Get-Content -LiteralPath ([IO.Path]::GetFullPath($ManifestPath)) -Raw | ConvertFrom-Json
$failures = @()
foreach ($item in $manifest.items) {
    $path = Join-Path $backup $item.storageKey
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { $failures += $item.storageKey; continue }
    $file = Get-Item -LiteralPath $path
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    if ($file.Length -ne [int64]$item.sizeBytes -or $hash -ne $item.sha256) { $failures += $item.storageKey }
}
$diskFiles = @(Get-ChildItem -LiteralPath $backup -File -Force)
if ($diskFiles.Count -ne [int]$manifest.fileCount -or $failures.Count -gt 0) { throw 'Backup verification failed.' }
[pscustomobject]@{ Status='ok'; FileCount=$diskFiles.Count; TotalBytes=[int64](($diskFiles | Measure-Object Length -Sum).Sum); Failed=0 } | ConvertTo-Json -Compress
