$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
New-Item -ItemType Directory -Force "$Root\logs" | Out-Null

function Test-Port([int]$Port) {
    try { return (Test-NetConnection 127.0.0.1 -Port $Port -InformationLevel Quiet -WarningAction SilentlyContinue) }
    catch { return $false }
}

# Neo4j 5.x needs an ASCII NEO4J_HOME on Windows.
$Drive = "K:"
if (Test-Path "$Drive\") {
    $existing = (subst | Select-String "^K:").ToString()
    if ($existing -notlike "*$Root*") { throw "K: is already used. Change Drive in start.ps1." }
} else {
    subst $Drive $Root
}

$Neo4j = "${Drive}\runtime\neo4j-community-5.26.28"
$Java = Get-ChildItem "${Drive}\runtime" -Directory | Where-Object Name -Like "zulu*" | Select-Object -First 1
$env:JAVA_HOME = $Java.FullName
$env:NEO4J_HOME = $Neo4j

if (-not (Test-Port 7687)) {
    $neoProcess = Start-Process powershell.exe -ArgumentList @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "$Neo4j\bin\neo4j.ps1", "console"
    ) -WorkingDirectory $Root -WindowStyle Hidden -RedirectStandardOutput "$Root\logs\neo4j.out.log" -RedirectStandardError "$Root\logs\neo4j.err.log" -PassThru
    $neoProcess.Id | Set-Content "$Root\logs\neo4j.pid"
    for ($i = 0; $i -lt 45 -and -not (Test-Port 7687); $i++) { Start-Sleep 1 }
    if (-not (Test-Port 7687)) { throw "Neo4j startup timed out. See logs/neo4j.err.log" }
}

$Python = "$Root\.venv\Scripts\python.exe"
if (-not (Test-Path $Python)) { throw "Python environment is missing. Run .\setup.ps1 first." }
if (-not (Test-Port 8000)) {
    $backend = Start-Process $Python -ArgumentList @("-m", "uvicorn", "app.main:app", "--host", "127.0.0.1", "--port", "8000") -WorkingDirectory "$Root\backend" -WindowStyle Hidden -RedirectStandardOutput "$Root\logs\backend.out.log" -RedirectStandardError "$Root\logs\backend.err.log" -PassThru
    $backend.Id | Set-Content "$Root\logs\backend.pid"
    for ($i = 0; $i -lt 30 -and -not (Test-Port 8000); $i++) { Start-Sleep 1 }
    if (-not (Test-Port 8000)) { throw "Backend startup timed out. See logs/backend.err.log" }
}

Write-Host "`nDynamic knowledge graph is running:" -ForegroundColor Green
Write-Host "  Graph UI:  http://127.0.0.1:8000"
Write-Host "  API docs:  http://127.0.0.1:8000/docs"
Write-Host "  Neo4j:     http://127.0.0.1:7474  (use credentials from .env)"
Start-Process "http://127.0.0.1:8000"
