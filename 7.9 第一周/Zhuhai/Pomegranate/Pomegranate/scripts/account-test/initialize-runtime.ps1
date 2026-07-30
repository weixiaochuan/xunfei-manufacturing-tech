param(
    [string] $ProjectRoot,
    [string] $RuntimeRoot,
    [string] $PostgresRoot
)

. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot

New-Item -ItemType Directory -Force -Path @(
    $context.RuntimeRoot,
    (Join-Path $context.RuntimeRoot 'logs'),
    $context.UserFilesRoot,
    $context.DesktopDataRoot
) | Out-Null

$settings = @"
`$script:AccountTestProjectRoot = '$($context.ProjectRoot.Replace("'", "''"))'
`$script:AccountTestRuntimeRoot = '$($context.RuntimeRoot.Replace("'", "''"))'
`$script:AccountTestPostgresRoot = '$($context.PostgresRoot.Replace("'", "''"))'
"@
[System.IO.File]::WriteAllText(
    (Join-Path $context.RuntimeRoot 'runtime-settings.ps1'),
    $settings,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Output "Account TEST runtime initialized outside Git: $($context.RuntimeRoot)"
Write-Output 'Add one value to each required secret file before starting Account Server:'
Write-Output "  $($context.CasdoorClientIdFile)"
Write-Output "  $($context.CasdoorClientSecretFile)"
