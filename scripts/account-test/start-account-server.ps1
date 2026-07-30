param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

. "$PSScriptRoot\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Ensure-AccountTestDirectories
Assert-AccountTestPrerequisites -RequireCasdoorSecrets -RequirePnpm
Set-AccountServerTestEnvironment

$logFile = Join-Path $script:LogRoot "account-server.log"
$errFile = Join-Path $script:LogRoot "account-server.err.log"
$pidFile = Join-Path $script:RuntimeRootPath "account-server.pid"
Stop-PidFileProcess -PidFile $pidFile
$pnpm = Get-RequiredCommand "pnpm.cmd"
$node = Get-RequiredCommand "node.exe"
& $pnpm --dir $script:AccountServerRoot run build
$process = Start-Process -FilePath $node -ArgumentList "dist/src/index.js" -WorkingDirectory $script:AccountServerRoot -RedirectStandardOutput $logFile -RedirectStandardError $errFile -PassThru -WindowStyle Hidden
Set-Content -LiteralPath $pidFile -Value $process.Id -NoNewline
Write-Host "Account Server TEST started on http://127.0.0.1:18080"
