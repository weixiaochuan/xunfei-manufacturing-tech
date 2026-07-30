param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
if (Test-Path -LiteralPath $context.DesktopPidFile -PathType Leaf) {
    $pidText = [System.IO.File]::ReadAllText($context.DesktopPidFile).Trim()
    if ($pidText -match '^\d+$') {
        $rootPid = [int]$pidText
        $all = Get-CimInstance Win32_Process
        $ids = New-Object System.Collections.Generic.List[int]
        $queue = New-Object System.Collections.Generic.Queue[int]
        $queue.Enqueue($rootPid)
        while ($queue.Count -gt 0) {
            $parent = $queue.Dequeue()
            foreach ($child in $all | Where-Object { $_.ParentProcessId -eq $parent }) {
                $queue.Enqueue([int]$child.ProcessId)
            }
            $ids.Add($parent)
        }
        foreach ($id in ($ids | Sort-Object -Descending)) {
            Stop-Process -Id $id -ErrorAction SilentlyContinue
        }
    }
    Remove-Item -LiteralPath $context.DesktopPidFile -Force
}
Write-Output 'Desktop TEST is stopped.'
