param(
    [string] $ProjectRoot,
    [string] $RuntimeRoot,
    [string] $PostgresRoot
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

function Get-RequiredCommand {
    param([Parameter(Mandatory = $true)] [string] $Name)
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "Required command is missing: $Name"
    }
    return $command
}

function Get-MsvcCompilerPath {
    $command = Get-Command cl.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
        return $null
    }
    $installationPath = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
    if ([string]::IsNullOrWhiteSpace($installationPath)) {
        return $null
    }
    $toolsRoot = Join-Path $installationPath.Trim() 'VC\Tools\MSVC'
    if (-not (Test-Path -LiteralPath $toolsRoot -PathType Container)) {
        return $null
    }
    $toolset = Get-ChildItem -LiteralPath $toolsRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1
    if (-not $toolset) {
        return $null
    }
    $compiler = Join-Path $toolset.FullName 'bin\Hostx64\x64\cl.exe'
    if (Test-Path -LiteralPath $compiler -PathType Leaf) {
        return $compiler
    }
    return $null
}

function Test-WebView2Runtime {
    $roots = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft\EdgeWebView\Application'),
        (Join-Path $env:LOCALAPPDATA 'Microsoft\EdgeWebView\Application')
    )
    foreach ($root in $roots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) {
            continue
        }
        $binary = Get-ChildItem -LiteralPath $root -Filter msedgewebview2.exe -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($binary) {
            return $true
        }
    }
    return $false
}

Write-Output 'Checking local development prerequisites...'
$git = Get-RequiredCommand -Name 'git.exe'
$node = Get-RequiredCommand -Name 'node.exe'
$pnpm = Get-RequiredCommand -Name 'pnpm.cmd'
$cargo = Get-RequiredCommand -Name 'cargo.exe'
$rustc = Get-RequiredCommand -Name 'rustc.exe'

$nodeVersion = (& $node.Source --version).Trim()
if ($nodeVersion -notmatch '^v22\.') {
    throw "Node.js 22 is required. Detected: $nodeVersion"
}
$msvcCompiler = Get-MsvcCompilerPath
if ([string]::IsNullOrWhiteSpace($msvcCompiler)) {
    throw 'Windows C++ Build Tools with the MSVC x64 compiler are required.'
}
if (-not (Test-WebView2Runtime)) {
    throw 'Microsoft Edge WebView2 Runtime is required but was not detected.'
}

$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
$postgres = Join-Path $context.PostgresBin 'postgres.exe'
if (-not (Test-Path -LiteralPath $postgres -PathType Leaf)) {
    throw "PostgreSQL server tool is missing: $postgres"
}
$postgresVersion = (& $postgres --version).Trim()
if ($postgresVersion -notmatch 'PostgreSQL\) 17\.') {
    throw "PostgreSQL 17 tools are required. Detected: $postgresVersion"
}

Write-Output "  Git: $((& $git.Source --version).Trim())"
Write-Output "  Node.js: $nodeVersion"
Write-Output "  pnpm: $((& $pnpm.Source --version).Trim())"
Write-Output "  Rust: $((& $rustc.Source --version).Trim())"
Write-Output "  Cargo: $((& $cargo.Source --version).Trim())"
Write-Output "  MSVC: detected"
Write-Output "  WebView2 Runtime: detected"
Write-Output "  PostgreSQL: $postgresVersion"

& (Join-Path $PSScriptRoot 'initialize-runtime.ps1') -ProjectRoot $context.ProjectRoot -RuntimeRoot $context.RuntimeRoot -PostgresRoot $context.PostgresRoot
& (Join-Path $PSScriptRoot 'initialize-postgres.ps1') -ProjectRoot $context.ProjectRoot -RuntimeRoot $context.RuntimeRoot -PostgresRoot $context.PostgresRoot

Write-Output 'Installing locked project dependencies...'
Push-Location $context.ProjectRoot
try {
    & pnpm.cmd install --frozen-lockfile
    Assert-AccountTestCommand 'pnpm dependency installation'

    $targetTriple = (& rustc.exe -vV | Select-String '^host:' | ForEach-Object { $_.Line.Split(':', 2)[1].Trim() })
    if ([string]::IsNullOrWhiteSpace($targetTriple)) {
        throw 'Cannot determine the Rust host target for the kb-mcp sidecar.'
    }
    $sidecar = Join-Path $context.ProjectRoot "src-tauri\binaries\kb-mcp-$targetTriple.exe"
    if (-not (Test-Path -LiteralPath $sidecar -PathType Leaf)) {
        Write-Output 'Generating the kb-mcp sidecar with the repository build flow...'
        & pnpm.cmd build:mcp:debug
        Assert-AccountTestCommand 'kb-mcp sidecar generation'
    } else {
        Write-Output 'kb-mcp sidecar already exists; leaving it unchanged.'
    }
    if (-not (Test-Path -LiteralPath $sidecar -PathType Leaf)) {
        throw "kb-mcp sidecar was not generated at the required location: $sidecar"
    }
} finally {
    Pop-Location
}

$credentialFiles = @(
    @{ Path = $context.CasdoorClientIdFile; Label = 'Casdoor TEST Client ID'; Format = 'one raw Client ID value, without KEY= or quotes' },
    @{ Path = $context.CasdoorClientSecretFile; Label = 'Casdoor TEST Client Secret'; Format = 'one raw Client Secret value, without KEY= or quotes' },
    @{ Path = $context.TestUsersFile; Label = 'Casdoor TEST users'; Format = 'one username=<TEST-only password> pair per line' }
)
$missing = @()
foreach ($credential in $credentialFiles) {
    $present = (Test-Path -LiteralPath $credential.Path -PathType Leaf) -and ((Get-Item -LiteralPath $credential.Path).Length -gt 0)
    if (-not $present) {
        $missing += $credential
    }
}

Write-Output ''
Write-Output 'Non-secret account TEST preparation is complete.'
Write-Output "Runtime: $($context.RuntimeRoot)"
Write-Output 'PostgreSQL was initialized but was not started.'
if ($missing.Count -gt 0) {
    Write-Warning 'The following TEST-only credential files are still required before start-all.ps1 can complete login:'
    foreach ($credential in $missing) {
        Write-Output "  $($credential.Label): $($credential.Path)"
        Write-Output "    Format: $($credential.Format)"
    }
    Write-Output 'No placeholder credential was generated. Add the real TEST values outside Git, then run start-all.ps1.'
} else {
    Write-Output 'All three TEST credential files exist and are non-empty. Their contents were not displayed.'
    Write-Output 'Next: run start-all.ps1 -StartDesktop.'
}
