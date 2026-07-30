param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
if (Test-Path -LiteralPath $context.AccountServerPidFile -PathType Leaf) {
    $pidText = [System.IO.File]::ReadAllText($context.AccountServerPidFile).Trim()
    if ($pidText -match '^\d+$') {
        $process = Get-Process -Id ([int]$pidText) -ErrorAction SilentlyContinue
        if ($process) {
            Stop-Process -Id $process.Id
            $process.WaitForExit(10000)
        }
    }
    Remove-Item -LiteralPath $context.AccountServerPidFile -Force
}
Write-Output 'Account Server TEST is stopped.'
