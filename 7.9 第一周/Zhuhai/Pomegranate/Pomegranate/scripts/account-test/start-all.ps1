param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot, [switch] $StartDesktop)
$forward = @{}
if ($ProjectRoot) { $forward.ProjectRoot = $ProjectRoot }
if ($RuntimeRoot) { $forward.RuntimeRoot = $RuntimeRoot }
if ($PostgresRoot) { $forward.PostgresRoot = $PostgresRoot }
& (Join-Path $PSScriptRoot 'start-postgres.ps1') @forward
& (Join-Path $PSScriptRoot 'start-account-server.ps1') @forward
& (Join-Path $PSScriptRoot 'check-runtime.ps1') @forward
if ($StartDesktop) {
    & (Join-Path $PSScriptRoot 'start-desktop.ps1') @forward -Background
}
