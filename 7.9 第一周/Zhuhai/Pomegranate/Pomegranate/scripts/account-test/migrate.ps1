param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& (Join-Path $PSScriptRoot 'start-postgres.ps1') -ProjectRoot $context.ProjectRoot -RuntimeRoot $context.RuntimeRoot -PostgresRoot $context.PostgresRoot
try {
    Set-AccountServerEnvironment -Context $context
    Push-Location $context.ProjectRoot
    try {
        & pnpm.cmd --filter '@pomegranate/account-server' migrate
        Assert-AccountTestCommand 'Account Server TEST migrations'
    } finally {
        Pop-Location
    }
} finally {
    Clear-AccountServerSecrets
}
