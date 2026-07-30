param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

. "$PSScriptRoot\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Ensure-AccountTestDirectories
Assert-AccountTestPrerequisites -RequirePnpm
Import-MsvcBuildEnvironment

$env:POMEGRANATE_DEPLOYMENT_PROFILE = "local"
$env:POMEGRANATE_ACCOUNT_SERVER_URL = "http://127.0.0.1:18080"
$env:POMEGRANATE_DESKTOP_DATA_DIR = $script:DesktopDataRoot
$env:KB_DATA_DIR = $script:DesktopDataRoot
$env:POMEGRANATE_FORCE_VISIBLE_DEV_WINDOW = "1"
$pnpm = Get-RequiredCommand "pnpm.cmd"
& $pnpm --dir $script:ProjectRootPath tauri dev
