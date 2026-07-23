param(
    [Parameter(Mandatory = $true)][string]$Source,
    [string]$BackupRoot = 'D:\PomegranateServer\backups\user-files',
    [string]$ManifestRoot = 'D:\PomegranateServer\backups\manifests'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$sourcePath = [IO.Path]::GetFullPath($Source)
if (-not (Test-Path -LiteralPath $sourcePath -PathType Container)) { throw 'Source must be an existing directory.' }
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
$target = Join-Path ([IO.Path]::GetFullPath($BackupRoot)) $timestamp
$manifestPath = Join-Path ([IO.Path]::GetFullPath($ManifestRoot)) "backup-$timestamp.json"
if (Test-Path -LiteralPath $target) { throw 'Timestamp backup target already exists.' }
New-Item -ItemType Directory -Path $target | Out-Null
New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($manifestPath)) | Out-Null
$items = @()
foreach ($file in Get-ChildItem -LiteralPath $sourcePath -File -Force) {
    $destination = Join-Path $target $file.Name
    Copy-Item -LiteralPath $file.FullName -Destination $destination
    $copied = Get-Item -LiteralPath $destination
    $sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash.ToLowerInvariant()
    $targetHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash.ToLowerInvariant()
    if ($file.Length -ne $copied.Length -or $sourceHash -ne $targetHash) { throw 'Backup verification failed.' }
    $items += [pscustomobject]@{ storageKey=$file.Name; sizeBytes=$file.Length; sha256=$sourceHash }
}
$manifest = [ordered]@{ version=1; createdAt=(Get-Date).ToUniversalTime().ToString('o'); source='filesystem:current'; target='filesystem:timestamp-backup'; fileCount=$items.Count; totalBytes=[int64](($items | Measure-Object sizeBytes -Sum).Sum); items=$items }
$json = $manifest | ConvertTo-Json -Depth 5
$stream = [IO.File]::Open($manifestPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
try { $writer=[IO.StreamWriter]::new($stream,[Text.UTF8Encoding]::new($false));$writer.Write($json);$writer.Flush();$stream.Flush($true);$writer.Dispose() } finally { $stream.Dispose() }
[pscustomobject]@{ BackupPath=$target; ManifestPath=$manifestPath; FileCount=$items.Count; TotalBytes=$manifest.totalBytes } | ConvertTo-Json -Compress
