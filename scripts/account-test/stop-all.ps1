param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

& "$PSScriptRoot\stop-desktop.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& "$PSScriptRoot\stop-account-server.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& "$PSScriptRoot\stop-postgres.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
