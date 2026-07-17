$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
if (-not (Test-Path "$Root\.venv\Scripts\python.exe")) { throw "Run setup.ps1 first." }
Push-Location "$Root\backend"
try { & "$Root\.venv\Scripts\python.exe" scripts\import_process_graph.py }
finally { Pop-Location }
