# win7_winrt_shim/build.ps1
# Build the WinRT shim DLL for Windows 7 compatibility.
#
# Usage: .\build.ps1 [-OutputDir ..\binaries]
# Output: api-ms-win-core-winrt-l1-1-0.dll
#
# Requires: Visual Studio Build Tools (cl.exe + link.exe in PATH)
# Run from: Developer Command Prompt for VS, or with VS environment loaded.

param(
    [string]$OutputDir = "..\binaries",
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "[shim-dll] Building winrt shim DLL..." -ForegroundColor Cyan

# Source files
$Sources = @(
    "$ScriptDir\dllmain.c",
    "$ScriptDir\hstring.c",
    "$ScriptDir\winrt_stubs.c"
)

$DefFile = "$ScriptDir\exports.def"
$OutName = "api-ms-win-core-winrt-l1-1-0.dll"

# Output directory
$OutDir = Join-Path $ScriptDir $OutputDir
if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}
$OutPath = Join-Path (Resolve-Path $OutDir) $OutName

# Object file directory (temp)
$ObjDir = "$ScriptDir\obj"
if (-not (Test-Path $ObjDir)) {
    New-Item -ItemType Directory -Path $ObjDir -Force | Out-Null
}

# Try to find cl.exe
$cl = Get-Command cl.exe -ErrorAction SilentlyContinue
if (-not $cl) {
    $vsBase = "${env:ProgramFiles}\Microsoft Visual Studio\2022"
    if (-not (Test-Path $vsBase)) { $vsBase = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2019" }
    if (Test-Path $vsBase) {
        $editions = @("Community", "Enterprise", "Professional")
        foreach ($ed in $editions) {
            $msvcDir = "$vsBase\$ed\VC\Tools\MSVC"
            if (Test-Path $msvcDir) {
                $latest = Get-ChildItem $msvcDir -Directory | Sort-Object Name -Descending | Select-Object -First 1
                if ($latest) {
                    $cl = Join-Path $latest.FullName "bin\Hostx64\x64\cl.exe"
                    break
                }
            }
        }
    }
    if (-not (Test-Path $cl)) {
        Write-Error "cl.exe not found. Run from Developer Command Prompt or set VS2022_PATH."
        exit 1
    }
}

$clPath = if ($cl -is [System.Management.Automation.CommandInfo]) { $cl.Source } else { $cl }

# Derive link.exe path from cl.exe path
$linkPath = Join-Path (Split-Path -Parent $clPath) "link.exe"
if (-not (Test-Path $linkPath)) {
    Write-Error "link.exe not found alongside cl.exe: $linkPath"
    exit 1
}

# Common compiler flags
$CFlags = @("/nologo", "/c", "/O2", "/GS-", "/DNDEBUG", "/DWIN32", "/D_WINDOWS")
if ($Configuration -eq "Release") {
    $CFlags += "/MT"
}

# ─── Step 1: Compile each source to .obj ────────
$ObjFiles = @()
foreach ($src in $Sources) {
    $objName = [System.IO.Path]::GetFileNameWithoutExtension($src) + ".obj"
    $objPath = Join-Path $ObjDir $objName
    $compileArgs = @($CFlags) + @("/Fo`"$objPath`"", "`"$src`"")
    Write-Host "[shim-dll] Compile: $clPath $($compileArgs -join ' ')"
    & $clPath @compileArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Compile failed for $src (exit $LASTEXITCODE)"
        exit $LASTEXITCODE
    }
    $ObjFiles += $objPath
}

# ─── Step 2: Link into DLL with exports ─────────
$LinkFlags = @(
    "/NOLOGO",
    "/DLL",
    "/MACHINE:X64",
    "/OPT:REF",
    "/OPT:ICF",
    "/DEF:`"$DefFile`"",
    "/OUT:`"$OutPath`""
) + $ObjFiles

Write-Host "[shim-dll] Link: $linkPath $($LinkFlags -join ' ')"
& $linkPath @LinkFlags

if ($LASTEXITCODE -ne 0) {
    Write-Error "Link failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

# Clean up .obj files
Remove-Item -Path $ObjFiles -Force -ErrorAction SilentlyContinue

Write-Host "[shim-dll] Built: $OutPath" -ForegroundColor Green

# ─── Build bcryptprimitives.dll shim (ProcessPrng) ───
$BcryptSrc = "$ScriptDir\bcrypt_stubs.c"
$BcryptDef = "$ScriptDir\bcrypt_exports.def"
$BcryptOut = Join-Path (Resolve-Path $OutDir) "bcryptprimitives.dll"

Write-Host "[shim-dll] Building bcryptprimitives.dll shim..." -ForegroundColor Cyan
$bcryptObj = Join-Path $ObjDir "bcrypt_stubs.obj"
& $clPath @CFlags "/Fo`"$bcryptObj`"" "`"$BcryptSrc`""
if ($LASTEXITCODE -ne 0) { Write-Error "bcrypt_stubs compile failed"; exit $LASTEXITCODE }

& $linkPath /NOLOGO /DLL /MACHINE:X64 /OPT:REF /OPT:ICF "/DEF:`"$BcryptDef`"" "/OUT:`"$BcryptOut`"" "`"$bcryptObj`""
if ($LASTEXITCODE -ne 0) { Write-Error "bcrypt link failed"; exit $LASTEXITCODE }
Remove-Item $bcryptObj -Force -ErrorAction SilentlyContinue
Write-Host "[shim-dll] Built: $BcryptOut" -ForegroundColor Green
