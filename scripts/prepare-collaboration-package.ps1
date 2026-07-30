param(
  [string]$OutputDir = "collab-packages",
  [string]$PackageName = ""
)

$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$rootPath = $root.Path.TrimEnd("\", "/")

if ([string]::IsNullOrWhiteSpace($PackageName)) {
  $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
  $PackageName = "ag-collaboration-$stamp.zip"
}

$outDirPath = Join-Path $rootPath $OutputDir
New-Item -ItemType Directory -Force -Path $outDirPath | Out-Null
$zipPath = Join-Path $outDirPath $PackageName

if (Test-Path -LiteralPath $zipPath) {
  Remove-Item -LiteralPath $zipPath -Force
}

$excludeDirs = @(
  ".git",
  "node_modules",
  ".pnpm-store",
  "dist",
  "build",
  "target",
  ".vite",
  ".turbo",
  "runtime",
  "pomegranate-local-test",
  "postgres-data",
  "user-files",
  "desktop-data",
  "logs",
  "collab-packages"
)

$excludeFileNames = @(
  ".env",
  ".env.local",
  ".env.development",
  ".env.production",
  ".DS_Store",
  "Thumbs.db"
)

$excludeExtensions = @(
  ".log",
  ".pid",
  ".tmp"
)

function Get-RelativePathForZip {
  param([string]$FullName)

  if ($FullName.Length -le $rootPath.Length) {
    return ""
  }

  return $FullName.Substring($rootPath.Length).TrimStart("\", "/").Replace("\", "/")
}

function Test-ExcludedItem {
  param([System.IO.FileSystemInfo]$Item)

  $relative = Get-RelativePathForZip $Item.FullName
  if ([string]::IsNullOrWhiteSpace($relative)) {
    return $false
  }

  $parts = $relative -split "/"
  foreach ($part in $parts) {
    if ($excludeDirs -contains $part) {
      return $true
    }
  }

  if (-not $Item.PSIsContainer) {
    if ($excludeFileNames -contains $Item.Name) {
      return $true
    }

    if ($Item.Name -like ".env.*" -and $Item.Name -ne ".env.example") {
      return $true
    }

    if ($excludeExtensions -contains $Item.Extension) {
      return $true
    }
  }

  return $false
}

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$files = Get-ChildItem -LiteralPath $rootPath -Recurse -Force -File | Where-Object {
  -not (Test-ExcludedItem $_)
}

if ($files.Count -eq 0) {
  throw "No files were selected for the collaboration package."
}

$zip = [System.IO.Compression.ZipFile]::Open($zipPath, [System.IO.Compression.ZipArchiveMode]::Create)
try {
  foreach ($file in $files) {
    $entryName = Get-RelativePathForZip $file.FullName
    [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
      $zip,
      $file.FullName,
      $entryName,
      [System.IO.Compression.CompressionLevel]::Optimal
    ) | Out-Null
  }
} finally {
  $zip.Dispose()
}

if (-not (Test-Path -LiteralPath $zipPath)) {
  throw "Package creation failed: $zipPath was not created."
}

Write-Host "Collaboration package created: $zipPath"
Write-Host "Files included: $($files.Count)"
