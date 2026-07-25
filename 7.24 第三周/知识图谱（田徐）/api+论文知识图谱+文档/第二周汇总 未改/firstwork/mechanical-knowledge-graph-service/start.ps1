$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
New-Item -ItemType Directory -Force "$Root\logs" | Out-Null

function Read-DotEnvValue([string]$Name) {
    $envPath = Join-Path $Root ".env"
    if (-not (Test-Path $envPath)) { return $null }
    $line = Get-Content $envPath | Where-Object { $_ -match "^\s*$Name\s*=" } | Select-Object -First 1
    if (-not $line) { return $null }
    return ($line -replace "^\s*$Name\s*=\s*", "").Trim().Trim('"')
}

function Test-Port([int]$Port) {
    try { return (Test-NetConnection 127.0.0.1 -Port $Port -InformationLevel Quiet -WarningAction SilentlyContinue) }
    catch { return $false }
}

$Python = "$Root\.venv\Scripts\python.exe"
if (-not (Test-Path $Python)) { throw "Python environment is missing. Run .\setup.ps1 first." }

$portValue = Read-DotEnvValue "BACKEND_PORT"
$BackendPort = if ($portValue) { [int]$portValue } else { 8000 }

if (-not (Test-Port $BackendPort)) {
    $backend = Start-Process $Python -ArgumentList @(
        "-m", "uvicorn", "app.main:app", "--host", "127.0.0.1", "--port", "$BackendPort"
    ) -WorkingDirectory "$Root\backend" -WindowStyle Hidden -RedirectStandardOutput "$Root\logs\backend.out.log" -RedirectStandardError "$Root\logs\backend.err.log" -PassThru
    $backend.Id | Set-Content "$Root\logs\backend.pid"
    for ($i = 0; $i -lt 30 -and -not (Test-Port $BackendPort); $i++) { Start-Sleep 1 }
    if (-not (Test-Port $BackendPort)) { throw "Backend startup timed out. See logs/backend.err.log" }
}

Write-Host "`nMechanical knowledge graph backend is running:" -ForegroundColor Green
Write-Host "  Graph UI:  http://127.0.0.1:$BackendPort"
Write-Host "  API docs:  http://127.0.0.1:$BackendPort/docs"
Write-Host "  Neo4j:     Use your external Neo4j or docker compose service from .env"
Start-Process "http://127.0.0.1:$BackendPort"
