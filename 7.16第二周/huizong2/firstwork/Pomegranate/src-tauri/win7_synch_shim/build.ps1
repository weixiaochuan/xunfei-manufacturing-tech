#!/usr/bin/env powershell
# Build api-ms-win-core-synch-l1-2-0.dll shim for Windows 7
# Requires MSVC cl.exe in PATH (from vcvars64.bat or similar)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$OutputDir = Join-Path $ScriptDir "..\binaries"
$SrcFile = Join-Path $ScriptDir "api-ms-win-core-synch-l1-2-0.c"
$DefFile = Join-Path $ScriptDir "exports.def"
$OutDll = Join-Path $OutputDir "api-ms-win-core-synch-l1-2-0.dll"

# Ensure output directory exists
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
}

Write-Host "[synch-shim] Building api-ms-win-core-synch-l1-2-0.dll..."

# Compile: /LD = DLL, /O2 = optimize, /GS- = no security cookie (smaller), /MT = static CRT
$cmd = "cl /nologo /LD /O2 /GS- /MT /Fe:`"$OutDll`" `"$SrcFile`" /link /DEF:`"$DefFile`" /NODEFAULTLIB:libcmt.lib"
Write-Host "[synch-shim] $cmd"
Invoke-Expression $cmd

if ($LASTEXITCODE -ne 0) {
    Write-Error "[synch-shim] Build FAILED"
    exit 1
}

if (Test-Path $OutDll) {
    $size = (Get-Item $OutDll).Length
    Write-Host "[synch-shim] Built: $OutDll ($($size) bytes)"
} else {
    Write-Error "[synch-shim] DLL not found at expected path"
    exit 1
}
