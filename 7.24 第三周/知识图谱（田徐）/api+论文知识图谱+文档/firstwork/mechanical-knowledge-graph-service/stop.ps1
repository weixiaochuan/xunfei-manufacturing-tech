$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot

$pidFile = "$Root\logs\backend.pid"
if (Test-Path $pidFile) {
    $processId = [int](Get-Content $pidFile)
    & taskkill.exe /PID $processId /T /F 2>$null | Out-Null
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
}

Write-Host "Mechanical knowledge graph backend stopped. External Neo4j/Docker services were not modified."
