param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& (Join-Path $PSScriptRoot 'initialize-postgres.ps1') -ProjectRoot $context.ProjectRoot -RuntimeRoot $context.RuntimeRoot -PostgresRoot $context.PostgresRoot

$pgCtl = Join-Path $context.PostgresBin 'pg_ctl.exe'
& $pgCtl status -D $context.PostgresData *> $null
if ($LASTEXITCODE -ne 0) {
    & $pgCtl start -D $context.PostgresData -l $context.PostgresLog -w
    Assert-AccountTestCommand 'PostgreSQL TEST startup'
}
& (Join-Path $context.PostgresBin 'pg_isready.exe') -h $context.PostgresHost -p $context.PostgresPort -d postgres -U $context.PostgresUser
Assert-AccountTestCommand 'PostgreSQL TEST readiness check'

$password = Read-AccountTestSecret -LiteralPath $context.PostgresPasswordFile -Label 'PostgreSQL TEST password'
$previous = $env:PGPASSWORD
try {
    $env:PGPASSWORD = $password
    $psql = Join-Path $context.PostgresBin 'psql.exe'
    $lookup = (& $psql -h $context.PostgresHost -p $context.PostgresPort -U $context.PostgresUser -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '$($context.PostgresDatabase)'" | Select-Object -First 1)
    Assert-AccountTestCommand 'PostgreSQL TEST database lookup'
    $exists = if ($null -eq $lookup) { '' } else { ([string]$lookup).Trim() }
    if ($exists -ne '1') {
        & $psql -h $context.PostgresHost -p $context.PostgresPort -U $context.PostgresUser -d postgres -v ON_ERROR_STOP=1 -c "CREATE DATABASE $($context.PostgresDatabase)"
        Assert-AccountTestCommand 'PostgreSQL TEST database creation'
    }
} finally {
    $env:PGPASSWORD = $previous
    $password = $null
}
Write-Output 'PostgreSQL TEST is ready on 127.0.0.1:55432.'
