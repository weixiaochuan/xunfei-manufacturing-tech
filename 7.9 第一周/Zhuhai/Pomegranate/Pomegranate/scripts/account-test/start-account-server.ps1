param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& (Join-Path $PSScriptRoot 'start-postgres.ps1') -ProjectRoot $context.ProjectRoot -RuntimeRoot $context.RuntimeRoot -PostgresRoot $context.PostgresRoot

if (Test-Path -LiteralPath $context.AccountServerPidFile -PathType Leaf) {
    $oldPid = [System.IO.File]::ReadAllText($context.AccountServerPidFile).Trim()
    if ($oldPid -match '^\d+$' -and (Get-Process -Id ([int]$oldPid) -ErrorAction SilentlyContinue)) {
        throw "Account Server TEST is already running with PID $oldPid."
    }
}

try {
    Set-AccountServerEnvironment -Context $context
    Push-Location $context.ProjectRoot
    try {
        & pnpm.cmd --filter '@pomegranate/account-server' build
        Assert-AccountTestCommand 'Account Server TEST build'
        & pnpm.cmd --filter '@pomegranate/account-server' migrate
        Assert-AccountTestCommand 'Account Server TEST migrations'
        $node = (Get-Command node.exe -ErrorAction Stop).Source
        $process = Start-Process -FilePath $node -ArgumentList 'dist/src/index.js' -WorkingDirectory $context.AccountServerRoot -WindowStyle Hidden -RedirectStandardOutput $context.AccountServerStdoutLog -RedirectStandardError $context.AccountServerStderrLog -PassThru
        [System.IO.File]::WriteAllText($context.AccountServerPidFile, [string]$process.Id, [System.Text.UTF8Encoding]::new($false))
    } finally {
        Pop-Location
    }
} finally {
    Clear-AccountServerSecrets
}

$deadline = [DateTime]::UtcNow.AddSeconds(30)
do {
    Start-Sleep -Milliseconds 500
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri "$($context.AccountServerPublicUrl)/health/ready" -TimeoutSec 2
        if ($response.StatusCode -eq 200) {
            Write-Output 'Account Server TEST is ready on 127.0.0.1:18080.'
            exit 0
        }
    } catch {}
} while ([DateTime]::UtcNow -lt $deadline)
throw 'Account Server TEST did not become ready. Inspect repository-external logs.'
