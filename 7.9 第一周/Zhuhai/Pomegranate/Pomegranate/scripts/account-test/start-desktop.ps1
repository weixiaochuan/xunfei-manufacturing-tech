param(
    [string] $ProjectRoot,
    [string] $RuntimeRoot,
    [string] $PostgresRoot,
    [switch] $Background
)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
try {
    $ready = Invoke-WebRequest -UseBasicParsing -Uri "$($context.AccountServerPublicUrl)/health/ready" -TimeoutSec 3
    if ($ready.StatusCode -ne 200) { throw 'not ready' }
} catch {
    throw 'Account Server TEST is not ready on 127.0.0.1:18080.'
}

$roaming = Join-Path $context.DesktopDataRoot 'Roaming'
$local = Join-Path $context.DesktopDataRoot 'Local'
New-Item -ItemType Directory -Force -Path $roaming, $local | Out-Null
$env:POMEGRANATE_DEPLOYMENT_PROFILE = 'local'
$env:POMEGRANATE_ACCOUNT_SERVER_URL = $context.AccountServerPublicUrl
$env:APPDATA = $roaming
$env:LOCALAPPDATA = $local

if (-not $Background) {
    Push-Location $context.ProjectRoot
    try { & pnpm.cmd tauri dev } finally { Pop-Location }
    exit $LASTEXITCODE
}

if (Test-Path -LiteralPath $context.DesktopPidFile -PathType Leaf) {
    $oldPid = [System.IO.File]::ReadAllText($context.DesktopPidFile).Trim()
    if ($oldPid -match '^\d+$' -and (Get-Process -Id ([int]$oldPid) -ErrorAction SilentlyContinue)) {
        throw "Desktop TEST launcher is already running with PID $oldPid."
    }
}
$command = "Set-Location -LiteralPath '$($context.ProjectRoot.Replace("'", "''"))'; pnpm.cmd tauri dev"
$process = Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-Command',$command) -WorkingDirectory $context.ProjectRoot -WindowStyle Hidden -RedirectStandardOutput $context.DesktopStdoutLog -RedirectStandardError $context.DesktopStderrLog -PassThru
[System.IO.File]::WriteAllText($context.DesktopPidFile, [string]$process.Id, [System.Text.UTF8Encoding]::new($false))
Write-Output "Desktop TEST launcher started with PID $($process.Id)."
