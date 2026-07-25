$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
New-Item -ItemType Directory -Force "$Root\logs" | Out-Null

$python = Get-Command python -ErrorAction SilentlyContinue
if (-not $python) { throw "Install Python 3.11 or 3.12 and ensure python is on PATH." }
& $python.Source -m venv "$Root\.venv"
& "$Root\.venv\Scripts\python.exe" -m pip install -e "$Root\backend[dev]"

Write-Host "Setup complete. Run .\start.ps1 next." -ForegroundColor Green
