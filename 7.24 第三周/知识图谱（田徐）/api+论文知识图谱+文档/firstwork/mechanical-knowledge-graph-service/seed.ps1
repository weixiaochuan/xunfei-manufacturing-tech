$ErrorActionPreference = "Stop"

param(
    [string]$Database = $env:NEO4J_DATABASE,
    [switch]$ConfirmReset,
    [switch]$AllowDefaultDatabase
)

$Root = $PSScriptRoot
$Python = "$Root\.venv\Scripts\python.exe"
if (-not (Test-Path $Python)) { throw "Run setup.ps1 first." }
if (-not $Database) {
    throw "Pass an explicit isolated -Database value, for example: -Database mechanical_process_graph."
}

if (-not $ConfirmReset) {
    throw "Refusing to import because it clears the target graph. Re-run with -ConfirmReset and an isolated -Database value."
}

$args = @("scripts\import_process_graph.py", "--database", $Database, "--confirm-reset")
if ($AllowDefaultDatabase) { $args += "--allow-default-database" }

Push-Location "$Root\backend"
try {
    & $Python @args
}
finally {
    Pop-Location
}
