param(
    [string]$ApiBaseUrl = 'https://api.stargathering.cn',
    [string]$AuthBaseUrl = 'https://auth.stargathering.cn',
    [string]$OutputDirectory = 'D:\PomegranateBuilds\cloud-test\20260723',
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'CloudClient.Common.ps1')

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$apiOrigin = Assert-CloudPublicOrigin `
    -Name 'ApiBaseUrl' `
    -Value $ApiBaseUrl `
    -ExpectedHost 'api.stargathering.cn'
$authOrigin = Assert-CloudPublicOrigin `
    -Name 'AuthBaseUrl' `
    -Value $AuthBaseUrl `
    -ExpectedHost 'auth.stargathering.cn'
Assert-OutputOutsideRepository -RepositoryRoot $repositoryRoot -OutputDirectory $OutputDirectory

if (-not (Test-Path -LiteralPath (Join-Path $repositoryRoot 'node_modules'))) {
    throw 'Dependencies are missing. Run pnpm install --frozen-lockfile in this worktree first.'
}

$previousProfile = [Environment]::GetEnvironmentVariable('POMEGRANATE_DEPLOYMENT_PROFILE', 'Process')
$previousApi = [Environment]::GetEnvironmentVariable('POMEGRANATE_ACCOUNT_SERVER_URL', 'Process')
$previousDocumentSource = [Environment]::GetEnvironmentVariable('VITE_DOCUMENT_SOURCE', 'Process')

Push-Location $repositoryRoot
try {
    $env:POMEGRANATE_DEPLOYMENT_PROFILE = 'cloud'
    $env:POMEGRANATE_ACCOUNT_SERVER_URL = $apiOrigin
    $env:VITE_DOCUMENT_SOURCE = 'account'

    if (-not $SkipTests) {
        & powershell -NoProfile -ExecutionPolicy Bypass `
            -File (Join-Path $PSScriptRoot 'test-cloud-client.ps1')
        if ($LASTEXITCODE -ne 0) {
            throw 'Cloud TEST public URL tests failed.'
        }

        & pnpm test:account-documents
        if ($LASTEXITCODE -ne 0) {
            throw 'Document adapter tests failed.'
        }

        & cargo test --manifest-path 'src-tauri\Cargo.toml' account_network::tests
        if ($LASTEXITCODE -ne 0) {
            throw 'Rust Cloud URL tests failed.'
        }
    }

    & pnpm tauri build --config 'src-tauri/tauri.cloud.conf.json' --bundles nsis
    if ($LASTEXITCODE -ne 0) {
        throw 'Cloud TEST NSIS build failed.'
    }

    $bundleDirectory = Join-Path $repositoryRoot 'src-tauri\target\release\bundle\nsis'
    $sourceInstaller = Get-ChildItem -LiteralPath $bundleDirectory -Filter '*.exe' -File |
        Where-Object { $_.Name -like '*Cloud*TEST*setup.exe' } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if ($null -eq $sourceInstaller) {
        throw 'The build completed but no Cloud TEST NSIS installer was found.'
    }

    $version = (Get-Content -LiteralPath 'src-tauri\tauri.conf.json' -Raw -Encoding UTF8 |
        ConvertFrom-Json).version
    $installerFileName = "Pomegranate Cloud TEST_${version}_x64-setup.exe"
    $absoluteOutput = [IO.Path]::GetFullPath($OutputDirectory)
    [IO.Directory]::CreateDirectory($absoluteOutput) | Out-Null
    $installerPath = Join-Path $absoluteOutput $installerFileName
    [IO.File]::Copy($sourceInstaller.FullName, $installerPath, $true)

    $installer = Get-Item -LiteralPath $installerPath
    $sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $shaFile = "$installerPath.sha256"
    [IO.File]::WriteAllText(
        $shaFile,
        "$sha256  $installerFileName`n",
        [Text.UTF8Encoding]::new($false)
    )

    $signature = Get-AuthenticodeSignature -LiteralPath $installerPath
    $codeSigned = $signature.Status -eq [Management.Automation.SignatureStatus]::Valid
    $manifest = [ordered]@{
        productName = 'Pomegranate Cloud TEST'
        version = $version
        platform = 'windows'
        architecture = 'x86_64'
        gitBranch = (& git branch --show-current).Trim()
        gitCommit = (& git rev-parse HEAD).Trim()
        builtAt = [DateTime]::UtcNow.ToString('o')
        apiBaseUrl = $apiOrigin
        authBaseUrl = $authOrigin
        deepLink = 'pomegranate://auth/callback'
        installerFileName = $installerFileName
        installerSizeBytes = $installer.Length
        installerSha256 = $sha256
        updaterMode = 'manual-installer'
        codeSigned = $codeSigned
        serverValidationStatus = 'WAITING_FOR_SERVER'
    }
    $manifestPath = Join-Path $absoluteOutput 'cloud-test-build-manifest.json'
    $manifestJson = $manifest | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText(
        $manifestPath,
        "$manifestJson`n",
        [Text.UTF8Encoding]::new($false)
    )

    Write-Host "Cloud TEST installer: $installerPath"
    Write-Host "SHA-256: $sha256"
    Write-Host "Public build manifest: $manifestPath"
} finally {
    Pop-Location
    [Environment]::SetEnvironmentVariable(
        'POMEGRANATE_DEPLOYMENT_PROFILE',
        $previousProfile,
        'Process'
    )
    [Environment]::SetEnvironmentVariable(
        'POMEGRANATE_ACCOUNT_SERVER_URL',
        $previousApi,
        'Process'
    )
    [Environment]::SetEnvironmentVariable(
        'VITE_DOCUMENT_SOURCE',
        $previousDocumentSource,
        'Process'
    )
}
