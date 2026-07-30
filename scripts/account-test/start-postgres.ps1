param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

. "$PSScriptRoot\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Ensure-AccountTestDirectories
Assert-AccountTestPrerequisites -RequirePostgresTools

if (!(Test-Path -LiteralPath $script:PostgresDataRoot)) {
  & "$PSScriptRoot\initialize-postgres.ps1" -ProjectRoot $script:ProjectRootPath -RuntimeRoot $script:RuntimeRootPath -PostgresRoot $script:PostgresRootPath
}

$pgCtl = Get-PostgresBin "pg_ctl.exe"
$createdb = Get-PostgresBin "createdb.exe"
$psql = Get-PostgresBin "psql.exe"
$logFile = Join-Path $script:LogRoot "postgres.log"
$env:PGPASSWORD = Read-SecretFile "postgres-password.tmp"

if (!(Test-TcpPort -HostName "127.0.0.1" -Port 55432)) {
  & $pgCtl -D $script:PostgresDataRoot -o "-h 127.0.0.1 -p 55432" -l $logFile start
}

$databaseExists = & $psql -h 127.0.0.1 -p 55432 -U pomegranate_account_test -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname = 'pomegranate_account_test'"
if ($databaseExists.Trim() -ne "1") {
  & $createdb -h 127.0.0.1 -p 55432 -U pomegranate_account_test pomegranate_account_test
}
Write-Host "PostgreSQL TEST is listening on 127.0.0.1:55432"
