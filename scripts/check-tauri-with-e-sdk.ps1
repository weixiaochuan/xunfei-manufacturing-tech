param(
  [string]$CargoArgs = "check --manifest-path src-tauri\Cargo.toml --lib"
)

$ErrorActionPreference = "Stop"

$vsDevCmd = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
$sdkRoot = "E:\ag-tools\Windows Kits\10"
$sdkVersion = "10.0.19041.0"
$vcRoot = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207"

if (-not (Test-Path -LiteralPath $vsDevCmd)) {
  throw "VsDevCmd.bat not found: $vsDevCmd"
}
if (-not (Test-Path -LiteralPath $sdkRoot)) {
  throw "Windows SDK root not found: $sdkRoot"
}

$envLines = cmd /c "`"$vsDevCmd`" -arch=x64 -host_arch=x64 >nul && set"
foreach ($line in $envLines) {
  $idx = $line.IndexOf("=")
  if ($idx -gt 0) {
    [Environment]::SetEnvironmentVariable($line.Substring(0, $idx), $line.Substring($idx + 1), "Process")
  }
}

$env:WindowsSdkDir = "$sdkRoot\"
$env:WindowsSDKVersion = "$sdkVersion\"
$env:INCLUDE = @(
  "$vcRoot\include",
  "$sdkRoot\Include\$sdkVersion\ucrt",
  "$sdkRoot\Include\$sdkVersion\um",
  "$sdkRoot\Include\$sdkVersion\shared",
  "$sdkRoot\Include\$sdkVersion\winrt",
  "$sdkRoot\Include\$sdkVersion\cppwinrt",
  $env:INCLUDE
) -join ";"
$env:LIB = @(
  "$vcRoot\lib\x64",
  "$sdkRoot\Lib\$sdkVersion\ucrt\x64",
  "$sdkRoot\Lib\$sdkVersion\um\x64",
  $env:LIB
) -join ";"
$env:PATH = "$sdkRoot\bin\$sdkVersion\x64;$env:PATH"

$parts = $CargoArgs -split " "
& cargo @parts
