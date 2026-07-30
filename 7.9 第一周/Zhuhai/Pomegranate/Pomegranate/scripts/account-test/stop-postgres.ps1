param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
$pgCtl = Join-Path $context.PostgresBin 'pg_ctl.exe'
& $pgCtl status -D $context.PostgresData *> $null
if ($LASTEXITCODE -eq 0) {
    & $pgCtl stop -D $context.PostgresData -m fast -w
    Assert-AccountTestCommand 'PostgreSQL TEST shutdown'
}
Write-Output 'PostgreSQL TEST is stopped; data remains intact.'
