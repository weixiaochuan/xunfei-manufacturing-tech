param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, 9999)]
    [int]$Run,
    [string]$SnapshotPath = "",
    [string]$DatabasePath = "C:\Users\weixiaochuan\AppData\Roaming\edu.bit.inb-dev\dev-app.db"
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($SnapshotPath)) {
    $SnapshotPath = Join-Path $PSScriptRoot "input_snapshot.json"
}
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$tauriRoot = Join-Path $repoRoot "src-tauri"
$resultPath = Join-Path $PSScriptRoot ("planning_run_{0}_result.json" -f $Run)
$consolePath = Join-Path $PSScriptRoot ("planning_run_{0}_console.log" -f $Run)

if (-not (Test-Path -LiteralPath $SnapshotPath -PathType Leaf)) {
    throw "Snapshot does not exist: $SnapshotPath"
}
if (-not (Test-Path -LiteralPath $DatabasePath -PathType Leaf)) {
    throw "Pomegranate database does not exist: $DatabasePath"
}

$env:POME_NATIVE_REAL_DB = $DatabasePath
$env:POME_NATIVE_DEBUG_SNAPSHOT = [System.IO.Path]::GetFullPath($SnapshotPath)
$env:POME_NATIVE_DEBUG_RESULT = [System.IO.Path]::GetFullPath($resultPath)
$env:POME_NATIVE_DEBUG_PLANNING_ONLY = "1"
$env:RUST_BACKTRACE = "1"

Push-Location $tauriRoot
try {
    $previousErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
        $previousNativePreference = $PSNativeCommandUseErrorActionPreference
        $PSNativeCommandUseErrorActionPreference = $false
    }
    & cargo test native_debug_loop_from_snapshot -- --ignored --nocapture --test-threads=1 2>&1 |
        Tee-Object -FilePath $consolePath
    $cargoExitCode = $LASTEXITCODE
}
finally {
    $ErrorActionPreference = $previousErrorActionPreference
    if (Test-Path Variable:previousNativePreference) {
        $PSNativeCommandUseErrorActionPreference = $previousNativePreference
    }
    Pop-Location
    Remove-Item Env:POME_NATIVE_REAL_DB -ErrorAction SilentlyContinue
    Remove-Item Env:POME_NATIVE_DEBUG_SNAPSHOT -ErrorAction SilentlyContinue
    Remove-Item Env:POME_NATIVE_DEBUG_RESULT -ErrorAction SilentlyContinue
    Remove-Item Env:POME_NATIVE_DEBUG_PLANNING_ONLY -ErrorAction SilentlyContinue
}

Write-Output "run=$Run"
Write-Output "cargoExitCode=$cargoExitCode"
Write-Output "result=$resultPath"
Write-Output "console=$consolePath"
exit $cargoExitCode
