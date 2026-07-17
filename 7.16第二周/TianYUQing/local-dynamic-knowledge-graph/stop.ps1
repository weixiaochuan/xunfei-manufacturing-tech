$Root = $PSScriptRoot
foreach ($name in @("backend", "neo4j")) {
    $pidFile = "$Root\logs\$name.pid"
    if (Test-Path $pidFile) {
        $processId = [int](Get-Content $pidFile)
        # /T also terminates Java or Python child processes started by the wrapper.
        & taskkill.exe /PID $processId /T /F 2>$null | Out-Null
        Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
    }
}
subst K: /D 2>$null
Write-Host "Local knowledge graph services stopped."
