param(
    [Parameter(Mandatory = $true)]
    [string]$ApiBaseUrl,
    [Parameter(Mandatory = $true)]
    [string]$AuthBaseUrl,
    [string]$OutputDirectory = 'D:\PomegranateBuilds\public-ip-test\output',
    [switch]$AllowInsecureHttp,
    [switch]$SkipTests,
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'PublicIpTest.Common.ps1')

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$tauriConfigPath = Join-Path $repositoryRoot 'src-tauri\tauri.public-ip-test.conf.json'
$apiOrigin = Assert-PublicIpTestOrigin `
    -Name 'ApiBaseUrl' `
    -Value $ApiBaseUrl `
    -AllowInsecureHttp:$AllowInsecureHttp
$authOrigin = Assert-PublicIpTestOrigin `
    -Name 'AuthBaseUrl' `
    -Value $AuthBaseUrl `
    -AllowInsecureHttp:$AllowInsecureHttp
$callbackUrl = "$apiOrigin/auth/callback"

Assert-PublicIpTestTauriConfig -ConfigPath $tauriConfigPath
Assert-OutputOutsideRepository `
    -RepositoryRoot $repositoryRoot `
    -OutputDirectory $OutputDirectory

Write-Host "Validated Account Server origin: $apiOrigin"
Write-Host "Validated Casdoor origin: $authOrigin"
Write-Host "Expected Account Server callback: $callbackUrl"
Write-Host 'Deployment profile: public-ip-test'
Write-Host 'Document source: account'

if ($ValidateOnly) {
    Write-Host 'Validation completed. No compilation or artifact generation was performed.'
    exit 0
}

if (-not (Test-Path -LiteralPath (Join-Path $repositoryRoot 'node_modules'))) {
    throw 'Dependencies are missing. Run pnpm install --frozen-lockfile in this worktree first.'
}

$previousProfile = [Environment]::GetEnvironmentVariable(
    'POMEGRANATE_DEPLOYMENT_PROFILE',
    'Process'
)
$previousApi = [Environment]::GetEnvironmentVariable(
    'POMEGRANATE_ACCOUNT_SERVER_URL',
    'Process'
)
$previousAllowHttp = [Environment]::GetEnvironmentVariable(
    'POMEGRANATE_ALLOW_INSECURE_PUBLIC_IP_HTTP',
    'Process'
)
$previousDocumentSource = [Environment]::GetEnvironmentVariable(
    'VITE_DOCUMENT_SOURCE',
    'Process'
)

Push-Location $repositoryRoot
try {
    $env:POMEGRANATE_DEPLOYMENT_PROFILE = 'public-ip-test'
    $env:POMEGRANATE_ACCOUNT_SERVER_URL = $apiOrigin
    $env:POMEGRANATE_ALLOW_INSECURE_PUBLIC_IP_HTTP = if ($AllowInsecureHttp) {
        'true'
    } else {
        'false'
    }
    $env:VITE_DOCUMENT_SOURCE = 'account'

    if (-not $SkipTests) {
        & powershell -NoProfile -ExecutionPolicy Bypass `
            -File (Join-Path $PSScriptRoot 'test-public-ip-test.ps1')
        if ($LASTEXITCODE -ne 0) {
            throw 'Public IP TEST URL tests failed.'
        }

        & pnpm test:account-documents
        if ($LASTEXITCODE -ne 0) {
            throw 'Document adapter tests failed.'
        }

        & cargo test --manifest-path 'src-tauri\Cargo.toml' account_network::tests
        if ($LASTEXITCODE -ne 0) {
            throw 'Rust Account Server URL tests failed.'
        }
    }

    & pnpm tauri build `
        --config 'src-tauri/tauri.public-ip-test.conf.json' `
        --bundles nsis
    if ($LASTEXITCODE -ne 0) {
        throw 'Public IP TEST NSIS build failed.'
    }

    $bundleDirectory = Join-Path $repositoryRoot 'src-tauri\target\release\bundle\nsis'
    $sourceInstaller = Get-ChildItem -LiteralPath $bundleDirectory -Filter '*.exe' -File |
        Where-Object { $_.Name -like '*Public*IP*TEST*setup.exe' } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if ($null -eq $sourceInstaller) {
        throw 'The build completed but no Public IP TEST NSIS installer was found.'
    }

    $version = (
        Get-Content -LiteralPath 'src-tauri\tauri.conf.json' -Raw -Encoding UTF8 |
            ConvertFrom-Json
    ).version
    $installerFileName = "Pomegranate Public IP TEST_${version}_x64-setup.exe"
    $absoluteOutput = [IO.Path]::GetFullPath($OutputDirectory)
    [IO.Directory]::CreateDirectory($absoluteOutput) | Out-Null
    $installerPath = Join-Path $absoluteOutput $installerFileName
    [IO.File]::Copy($sourceInstaller.FullName, $installerPath, $true)

    $installer = Get-Item -LiteralPath $installerPath
    $sha256 = (
        Get-FileHash -LiteralPath $installerPath -Algorithm SHA256
    ).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText(
        "$installerPath.sha256",
        "$sha256  $installerFileName`n",
        [Text.UTF8Encoding]::new($false)
    )

    $signature = Get-AuthenticodeSignature -LiteralPath $installerPath
    $manifest = [ordered]@{
        productName = 'Pomegranate Public IP TEST'
        version = $version
        platform = 'windows'
        architecture = 'x86_64'
        gitBranch = (& git branch --show-current).Trim()
        gitCommit = (& git rev-parse HEAD).Trim()
        builtAt = [DateTime]::UtcNow.ToString('o')
        apiBaseUrl = $apiOrigin
        authBaseUrl = $authOrigin
        callbackUrl = $callbackUrl
        deploymentProfile = 'public-ip-test'
        temporaryInsecureTransport = (
            $apiOrigin.StartsWith('http://', [StringComparison]::Ordinal) -or
            $authOrigin.StartsWith('http://', [StringComparison]::Ordinal)
        )
        deepLink = 'pomegranate://auth/callback'
        installerFileName = $installerFileName
        installerSizeBytes = $installer.Length
        installerSha256 = $sha256
        updaterMode = 'disabled'
        codeSigned = (
            $signature.Status -eq [Management.Automation.SignatureStatus]::Valid
        )
        serverValidationStatus = 'WAITING_FOR_SERVER'
    }
    $manifestPath = Join-Path $absoluteOutput 'public-ip-test-build-manifest.json'
    $manifestJson = $manifest | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText(
        $manifestPath,
        "$manifestJson`n",
        [Text.UTF8Encoding]::new($false)
    )

    Write-Host "Public IP TEST installer: $installerPath"
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
        'POMEGRANATE_ALLOW_INSECURE_PUBLIC_IP_HTTP',
        $previousAllowHttp,
        'Process'
    )
    [Environment]::SetEnvironmentVariable(
        'VITE_DOCUMENT_SOURCE',
        $previousDocumentSource,
        'Process'
    )
}
