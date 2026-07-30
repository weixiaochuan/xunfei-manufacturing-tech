param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

. "$PSScriptRoot\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Ensure-AccountTestDirectories
Get-PostgresBin "initdb.exe" | Out-Null

$initdb = Get-PostgresBin "initdb.exe"
$passwordFile = Join-Path $script:RuntimeRootPath "postgres-password.tmp"
if (!(Test-Path -LiteralPath $passwordFile)) {
  throw "Create local password file first: $passwordFile"
}
if (Test-Path -LiteralPath $script:PostgresDataRoot) {
  Write-Host "PostgreSQL data directory already exists: $script:PostgresDataRoot"
  exit 0
}

& $initdb -D $script:PostgresDataRoot -U "pomegranate_account_test" --encoding=UTF8 --auth=scram-sha-256 --pwfile=$passwordFile
