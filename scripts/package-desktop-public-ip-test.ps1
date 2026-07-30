param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql",
  [switch]$SkipMcpBuild
)

. "$PSScriptRoot\account-test\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Import-MsvcBuildEnvironment
Assert-AccountTestPrerequisites -RequirePnpm

function Invoke-Checked {
  param(
    [Parameter(Mandatory=$true)][string]$FilePath,
    [Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments
  )
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
  }
}

$env:POMEGRANATE_DEPLOYMENT_PROFILE = "public-ip-test"
$env:POMEGRANATE_ACCOUNT_SERVER_URL = "http://82.157.119.201:8080"
$env:POMEGRANATE_ALLOW_INSECURE_PUBLIC_IP_HTTP = "true"
$env:TAURI_SIGNING_PRIVATE_KEY = ""
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""

Set-Location -LiteralPath $script:ProjectRootPath

if (!$SkipMcpBuild) {
  Invoke-Checked "pnpm" "build:mcp"
}

Invoke-Checked "pnpm" "build"
Invoke-Checked "pnpm" "tauri" "build" "--bundles" "nsis" "--config" "src-tauri/tauri.public-ip-test.conf.json"

$bundleDir = Join-Path $script:ProjectRootPath "src-tauri\target\release\bundle\nsis"
Write-Host ""
Write-Host "NSIS installers:"
Get-ChildItem -LiteralPath $bundleDir -Filter "*.exe" -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending |
  Select-Object FullName, Length, LastWriteTime |
  Format-Table -AutoSize
