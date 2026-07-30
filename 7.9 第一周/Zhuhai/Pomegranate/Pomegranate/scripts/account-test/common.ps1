$ErrorActionPreference = 'Stop'

function Resolve-AccountTestContext {
    param(
        [string] $ProjectRoot,
        [string] $RuntimeRoot,
        [string] $PostgresRoot
    )

    if ([string]::IsNullOrWhiteSpace($ProjectRoot)) {
        $ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    }
    $ProjectRoot = [System.IO.Path]::GetFullPath($ProjectRoot)
    if (-not (Test-Path -LiteralPath (Join-Path $ProjectRoot 'services\account-server\package.json') -PathType Leaf)) {
        throw "ProjectRoot does not contain Account Server: $ProjectRoot"
    }

    if ([string]::IsNullOrWhiteSpace($RuntimeRoot)) {
        $gitRoot = (& git -C $ProjectRoot rev-parse --show-toplevel 2>$null | Select-Object -First 1)
        if ([string]::IsNullOrWhiteSpace($gitRoot)) {
            throw 'Cannot determine Git worktree root. Pass -RuntimeRoot explicitly.'
        }
        $RuntimeRoot = Join-Path (Split-Path -Parent $gitRoot.Trim()) 'runtime'
    }
    $RuntimeRoot = [System.IO.Path]::GetFullPath($RuntimeRoot)

    $settingsPath = Join-Path $RuntimeRoot 'runtime-settings.ps1'
    if (Test-Path -LiteralPath $settingsPath -PathType Leaf) {
        . $settingsPath
        if ([string]::IsNullOrWhiteSpace($PostgresRoot) -and $script:AccountTestPostgresRoot) {
            $PostgresRoot = $script:AccountTestPostgresRoot
        }
    }
    if ([string]::IsNullOrWhiteSpace($PostgresRoot)) {
        $PostgresRoot = $env:POMEGRANATE_TEST_POSTGRES_ROOT
    }
    if ([string]::IsNullOrWhiteSpace($PostgresRoot)) {
        $pgCtl = Get-Command pg_ctl.exe -ErrorAction SilentlyContinue
        if ($pgCtl) {
            $PostgresRoot = Split-Path -Parent (Split-Path -Parent $pgCtl.Source)
        }
    }
    if ([string]::IsNullOrWhiteSpace($PostgresRoot)) {
        throw 'PostgreSQL tools were not found. Pass -PostgresRoot or set POMEGRANATE_TEST_POSTGRES_ROOT.'
    }
    $PostgresRoot = [System.IO.Path]::GetFullPath($PostgresRoot)
    $PostgresBin = Join-Path $PostgresRoot 'bin'
    foreach ($tool in 'initdb.exe','pg_ctl.exe','pg_isready.exe','psql.exe') {
        if (-not (Test-Path -LiteralPath (Join-Path $PostgresBin $tool) -PathType Leaf)) {
            throw "PostgreSQL tool is missing: $tool under $PostgresBin"
        }
    }

    return [pscustomobject]@{
        ProjectRoot = $ProjectRoot
        RuntimeRoot = $RuntimeRoot
        AccountServerRoot = Join-Path $ProjectRoot 'services\account-server'
        PostgresRoot = $PostgresRoot
        PostgresBin = $PostgresBin
        PostgresData = Join-Path $RuntimeRoot 'postgres-data'
        PostgresLog = Join-Path $RuntimeRoot 'logs\postgres.log'
        PostgresPasswordFile = Join-Path $RuntimeRoot 'postgres-password.tmp'
        CasdoorClientIdFile = Join-Path $RuntimeRoot 'casdoor-client-id.tmp'
        CasdoorClientSecretFile = Join-Path $RuntimeRoot 'casdoor-client-secret.tmp'
        TestUsersFile = Join-Path $RuntimeRoot 'test-users.tmp'
        AccountServerPidFile = Join-Path $RuntimeRoot 'account-server.pid'
        DesktopPidFile = Join-Path $RuntimeRoot 'desktop.pid'
        AccountServerStdoutLog = Join-Path $RuntimeRoot 'logs\account-server.stdout.log'
        AccountServerStderrLog = Join-Path $RuntimeRoot 'logs\account-server.stderr.log'
        DesktopStdoutLog = Join-Path $RuntimeRoot 'logs\desktop.stdout.log'
        DesktopStderrLog = Join-Path $RuntimeRoot 'logs\desktop.stderr.log'
        DesktopDataRoot = Join-Path $RuntimeRoot 'desktop-data'
        UserFilesRoot = Join-Path $RuntimeRoot 'user-files'
        PostgresHost = '127.0.0.1'
        PostgresPort = 55432
        PostgresUser = 'pomegranate_test_admin'
        PostgresDatabase = 'pomegranate_account_test'
        AccountServerHost = '127.0.0.1'
        AccountServerPort = 18080
        AccountServerPublicUrl = 'http://127.0.0.1:18080'
        CasdoorPublicUrl = 'http://82.157.119.201:18000'
        CasdoorRedirectUri = 'http://127.0.0.1:18080/auth/callback'
        CasdoorOrganization = 'pomegranate-test'
        CasdoorApplication = 'app-pomegranate-test'
    }
}

function Read-AccountTestSecret {
    param(
        [Parameter(Mandatory = $true)] [string] $LiteralPath,
        [Parameter(Mandatory = $true)] [string] $Label
    )
    if (-not (Test-Path -LiteralPath $LiteralPath -PathType Leaf)) {
        throw "$Label file is missing: $LiteralPath"
    }
    $value = [System.IO.File]::ReadAllText($LiteralPath).Trim()
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "$Label file is empty: $LiteralPath"
    }
    return $value
}

function Assert-AccountTestCommand {
    param([Parameter(Mandatory = $true)] [string] $Action)
    if ($LASTEXITCODE -ne 0) {
        throw "$Action failed with exit code $LASTEXITCODE."
    }
}

function Set-AccountServerEnvironment {
    param([Parameter(Mandatory = $true)] $Context)
    $clientId = Read-AccountTestSecret -LiteralPath $Context.CasdoorClientIdFile -Label 'Casdoor TEST Client ID'
    $clientSecret = Read-AccountTestSecret -LiteralPath $Context.CasdoorClientSecretFile -Label 'Casdoor TEST Client Secret'
    $dbPassword = Read-AccountTestSecret -LiteralPath $Context.PostgresPasswordFile -Label 'PostgreSQL TEST password'

    $env:DEPLOYMENT_PROFILE = 'local'
    $env:ACCOUNT_SERVER_HOST = $Context.AccountServerHost
    $env:ACCOUNT_SERVER_PORT = [string]$Context.AccountServerPort
    $env:ACCOUNT_SERVER_PUBLIC_URL = $Context.AccountServerPublicUrl
    $env:ACCOUNT_DB_HOST = $Context.PostgresHost
    $env:ACCOUNT_DB_PORT = [string]$Context.PostgresPort
    $env:ACCOUNT_DB_NAME = $Context.PostgresDatabase
    $env:ACCOUNT_DB_USER = $Context.PostgresUser
    $env:ACCOUNT_DB_PASSWORD = $dbPassword
    $env:NODE_ENV = 'development'
    $env:OIDC_DEBUG_CLAIM_TYPES = 'false'
    $env:CASDOOR_PUBLIC_URL = $Context.CasdoorPublicUrl
    $env:CASDOOR_CLIENT_ID = $clientId
    $env:CASDOOR_CLIENT_SECRET = $clientSecret
    $env:CASDOOR_REDIRECT_URI = $Context.CasdoorRedirectUri
    $env:CASDOOR_ORGANIZATION = $Context.CasdoorOrganization
    $env:CASDOOR_APPLICATION = $Context.CasdoorApplication
    $env:ALLOW_LOCAL_TEST_CASDOOR = 'true'
    # The TEST Casdoor host currently signs tokens about eight hours ahead of UTC.
    # Account Server accepts this only for local + explicit TEST + the exact TEST origin.
    $env:CASDOOR_NBF_CLOCK_TOLERANCE_SECONDS = '28860'
    $env:FILE_STORAGE_BACKEND = 'filesystem'
    $env:USER_FILES_ROOT = $Context.UserFilesRoot
    $env:FILE_STORAGE_ALLOW_LEGACY_ROLLBACK = 'false'
    $env:USER_FILE_MAX_BYTES = '20971520'
}

function Clear-AccountServerSecrets {
    $env:CASDOOR_CLIENT_ID = $null
    $env:CASDOOR_CLIENT_SECRET = $null
    $env:ACCOUNT_DB_PASSWORD = $null
}
