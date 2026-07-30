param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

. "$PSScriptRoot\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Ensure-AccountTestDirectories
Assert-AccountTestPrerequisites -RequireCasdoorSecrets -RequirePnpm
Set-AccountServerTestEnvironment

$pnpm = Get-RequiredCommand "pnpm.cmd"
& $pnpm --dir $script:AccountServerRoot run migrate
