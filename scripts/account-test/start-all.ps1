param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql",
  [switch]$StartDesktop
)

& "$PSScriptRoot\start-postgres.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& "$PSScriptRoot\migrate.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& "$PSScriptRoot\start-account-server.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Start-Sleep -Seconds 2
& "$PSScriptRoot\check-runtime.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
if ($StartDesktop) {
  & "$PSScriptRoot\start-desktop.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
}
