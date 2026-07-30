param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
$forward = @{}
if ($ProjectRoot) { $forward.ProjectRoot = $ProjectRoot }
if ($RuntimeRoot) { $forward.RuntimeRoot = $RuntimeRoot }
if ($PostgresRoot) { $forward.PostgresRoot = $PostgresRoot }
& (Join-Path $PSScriptRoot 'stop-desktop.ps1') @forward
& (Join-Path $PSScriptRoot 'stop-account-server.ps1') @forward
& (Join-Path $PSScriptRoot 'stop-postgres.ps1') @forward
