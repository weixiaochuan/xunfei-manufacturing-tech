param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

. "$PSScriptRoot\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
$pgCtl = Get-PostgresBin "pg_ctl.exe"
if (Test-Path -LiteralPath $script:PostgresDataRoot) {
  & $pgCtl -D $script:PostgresDataRoot stop -m fast
}
