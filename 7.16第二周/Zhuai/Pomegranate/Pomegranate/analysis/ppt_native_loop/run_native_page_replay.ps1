param(
    [Parameter(Mandatory = $true)][string]$Project,
    [Parameter(Mandatory = $true)][ValidateRange(1, 99)][int]$Page,
    [switch]$ApplyFix
)

$ErrorActionPreference = "Stop"
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$env:PYTHONUTF8 = "1"
$env:PYTHONIOENCODING = "utf-8"

function Write-Utf8Atomic([string]$Path, [string]$Content) {
    $directory = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($directory)) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }
    $temporary = "$Path.$PID.tmp"
    [System.IO.File]::WriteAllText($temporary, $Content, [System.Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporary -Destination $Path -Force
}

function Invoke-Native([string]$Executable, [string[]]$Arguments) {
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $output = & $Executable @Arguments 2>&1 | Out-String
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    return [ordered]@{ exitCode = $exitCode; output = $output.Trim() }
}

function Get-GeometryReportSummary([string]$Python, [string]$Path) {
    $code = @'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    report = json.load(handle)

summary = {
    "hardErrorCount": len(report.get("hardErrors", [])),
    "passed": bool(report.get("passed", False)),
    "visibleTexts": [str(block.get("text", "")) for block in report.get("textBlocks", [])],
}
print(json.dumps(summary, ensure_ascii=True, separators=(",", ":")))
'@
    $encodedCode = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($code))
    $result = Invoke-Native $Python @(
        "-c", "import base64,sys;exec(base64.b64decode(sys.argv.pop(1)))", $encodedCode, $Path
    )
    if ($result.exitCode -ne 0) {
        throw "Unable to read geometry report: $Path`n$($result.output)"
    }
    return $result.output | ConvertFrom-Json
}

function Get-SvgVisibleTextSignature([string]$Python, [string]$Path) {
    $code = @'
import json
import sys
import xml.etree.ElementTree as ET

root = ET.parse(sys.argv[1]).getroot()
signature = []
for element in root.iter():
    if element.tag.rsplit("}", 1)[-1] != "text":
        continue
    segments = []
    if element.text and element.text.strip():
        segments.append(element.text)
    for child in list(element):
        value = "".join(child.itertext())
        if value:
            segments.append(value)
        if child.tail and child.tail.strip():
            segments.append(child.tail)
    signature.append(segments)
print(json.dumps(signature, ensure_ascii=True, separators=(",", ":")))
'@
    $encodedCode = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($code))
    $result = Invoke-Native $Python @(
        "-c", "import base64,sys;exec(base64.b64decode(sys.argv.pop(1)))", $encodedCode, $Path
    )
    if ($result.exitCode -ne 0) {
        throw "Unable to read visible SVG text: $Path`n$($result.output)"
    }
    return $result.output | ConvertFrom-Json
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$workspaceRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot "..\.."))
$pptMasterRoot = Join-Path $workspaceRoot "ppt-master"
$projectPath = [System.IO.Path]::GetFullPath($Project)
$pageId = "P{0:D2}" -f $Page
$pagePrefix = "{0:D2}_" -f $Page

$geometryChecker = Join-Path $repoRoot "src-tauri\scripts\ppt_native_text_geometry.py"
$powerPointChecker = Join-Path $repoRoot "src-tauri\scripts\ppt_native_powerpoint_geometry.ps1"
$python = Join-Path $pptMasterRoot ".venv\Scripts\python.exe"
$qualityChecker = Join-Path $pptMasterRoot "skills\ppt-master\scripts\svg_quality_checker.py"
$exporter = Join-Path $pptMasterRoot "skills\ppt-master\scripts\svg_to_pptx.py"

foreach ($required in @(
    (Join-Path $projectPath "deck_outline.json"),
    (Join-Path $projectPath "slide_plan.json"),
    (Join-Path $projectPath "slide_specs\slide-$('{0:D2}' -f $Page).json"),
    $geometryChecker,
    $powerPointChecker,
    $python,
    $qualityChecker,
    $exporter
)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required replay input does not exist: $required"
    }
}

$svgFiles = @(Get-ChildItem -LiteralPath (Join-Path $projectPath "svg_output") -File -Filter "$pagePrefix*.svg")
if ($svgFiles.Count -ne 1) {
    throw "Expected exactly one source SVG for $pageId, found $($svgFiles.Count)."
}
$sourceSvg = $svgFiles[0].FullName
$replayRoot = Join-Path $projectPath "analysis\native_page_replay\$pageId"
$stageProject = Join-Path $replayRoot "powerpoint_stage"
$stageSvgDir = Join-Path $stageProject "svg_output"
$workingSvg = Join-Path $replayRoot "working.svg"
$originalSvg = Join-Path $replayRoot "original.svg"
$beforeReport = Join-Path $replayRoot "geometry_before.json"
$afterReport = Join-Path $replayRoot "geometry_after.json"
$finalReport = Join-Path $replayRoot "geometry_after_powerpoint.json"
$qualityLog = Join-Path $replayRoot "svg_quality_checker.log"
$powerPointReport = Join-Path $replayRoot "powerpoint_geometry.json"
$pptxPath = Join-Path $replayRoot "$pageId-native-replay.pptx"

New-Item -ItemType Directory -Force -Path $replayRoot, $stageSvgDir | Out-Null
Copy-Item -LiteralPath $sourceSvg -Destination $originalSvg -Force
Copy-Item -LiteralPath $sourceSvg -Destination $workingSvg -Force
$beforeVisible = @(Get-SvgVisibleTextSignature $python $originalSvg)

$outline = Get-Content -LiteralPath (Join-Path $projectPath "deck_outline.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$slideSpecPath = Join-Path $projectPath "slide_specs\slide-$('{0:D2}' -f $Page).json"
$slideSpec = Get-Content -LiteralPath $slideSpecPath -Raw -Encoding UTF8 | ConvertFrom-Json
$slidePlan = Get-Content -LiteralPath (Join-Path $projectPath "slide_plan.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$pagePlan = @($slidePlan.slides | Where-Object {
    $_.page -eq $Page -or $_.page_index -eq $Page -or $_.pageIndex -eq $Page
}) | Select-Object -First 1
if ($null -eq $pagePlan) { throw "slide_plan.json does not contain $pageId" }

$capturedInputs = @(Get-ChildItem -LiteralPath (Join-Path $projectPath "analysis\native_executor_inputs") -File -Filter "$pageId-*.txt" -ErrorAction SilentlyContinue)
$executorInputSource = if ($capturedInputs.Count -gt 0) { $capturedInputs[0].FullName } else { "reconstructed-from-SlideSpec-and-SlidePlan" }
$executorInput = [ordered]@{
    page = $Page
    source = $executorInputSource
    deckOutlineSlide = @($outline.slides | Where-Object { $_.index -eq $Page }) | Select-Object -First 1
    slideSpec = $slideSpec
    retrievedMaterial = @($slideSpec.evidence)
    slidePlanPage = $pagePlan
}
Write-Utf8Atomic (Join-Path $replayRoot "executor_input.json") ($executorInput | ConvertTo-Json -Depth 20)
Write-Utf8Atomic (Join-Path $replayRoot "retrieved_material.json") (@($slideSpec.evidence) | ConvertTo-Json -Depth 8)

$before = Invoke-Native $python @($geometryChecker, "--svg", $workingSvg, "--report", $beforeReport)
Write-Utf8Atomic (Join-Path $replayRoot "geometry_before_console.log") $before.output

$repair = Invoke-Native $python @($geometryChecker, "--svg", $workingSvg, "--auto-fix", "--report", $afterReport)
Write-Utf8Atomic (Join-Path $replayRoot "geometry_repair_console.log") $repair.output
if ($repair.exitCode -ne 0) {
    throw "Deterministic page repair did not pass. Report: $afterReport"
}

$beforeSummary = Get-GeometryReportSummary $python $beforeReport
$afterSummary = Get-GeometryReportSummary $python $afterReport
$afterVisible = @(Get-SvgVisibleTextSignature $python $workingSvg)
if (($beforeVisible | ConvertTo-Json -Compress) -ne ($afterVisible | ConvertTo-Json -Compress)) {
    throw "Visible text changed during deterministic replay."
}

$quality = Invoke-Native $python @($qualityChecker, $workingSvg, "--format", "ppt169")
Write-Utf8Atomic $qualityLog $quality.output
if ($quality.exitCode -ne 0) { throw "SVG Quality Checker failed: $qualityLog" }

Get-ChildItem -LiteralPath $stageSvgDir -File -ErrorAction SilentlyContinue | Remove-Item -Force
$stageSvg = Join-Path $stageSvgDir $svgFiles[0].Name
Copy-Item -LiteralPath $workingSvg -Destination $stageSvg -Force
foreach ($optional in @("spec_lock.md", "design_spec.md")) {
    $candidate = Join-Path $projectPath $optional
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        Copy-Item -LiteralPath $candidate -Destination (Join-Path $stageProject $optional) -Force
    }
}

$export = Invoke-Native $python @(
    $exporter, $stageProject, "--output", $pptxPath, "--only", "native",
    "--no-notes", "--no-cache", "--workers", "1", "--transition", "none", "--animation", "none"
)
Write-Utf8Atomic (Join-Path $replayRoot "svg_to_pptx.log") $export.output
if ($export.exitCode -ne 0 -or -not (Test-Path -LiteralPath $pptxPath -PathType Leaf)) {
    throw "SVG to PPTX replay export failed."
}

$powerPoint = Invoke-Native "powershell.exe" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $powerPointChecker,
    "-PptxPath", $pptxPath, "-SvgDir", $stageSvgDir,
    "-RenderDir", (Join-Path $replayRoot "powerpoint_render"), "-ApplySafeRegionFixes"
)
Write-Utf8Atomic $powerPointReport $powerPoint.output
if ($powerPoint.exitCode -ne 0) { throw "PowerPoint geometry replay failed: $powerPointReport" }
$powerPointSummary = $powerPoint.output | ConvertFrom-Json

Copy-Item -LiteralPath $stageSvg -Destination $workingSvg -Force
$final = Invoke-Native $python @($geometryChecker, "--svg", $workingSvg, "--auto-fix", "--report", $finalReport)
if ($final.exitCode -ne 0) { throw "Final local geometry recheck failed: $finalReport" }
$finalSummary = Get-GeometryReportSummary $python $finalReport
$finalVisible = @(Get-SvgVisibleTextSignature $python $workingSvg)
if (($beforeVisible | ConvertTo-Json -Compress) -ne ($finalVisible | ConvertTo-Json -Compress)) {
    throw "Visible text changed after PowerPoint geometry replay."
}

if ($ApplyFix) {
    Copy-Item -LiteralPath $workingSvg -Destination $sourceSvg -Force
}

$summary = [ordered]@{
    schemaVersion = 1
    project = $projectPath
    page = $Page
    sourceSvg = $sourceSvg
    workingSvg = $workingSvg
    executorInputSource = $executorInputSource
    aiCalled = $false
    beforeHardErrors = $beforeSummary.hardErrorCount
    afterHardErrors = $finalSummary.hardErrorCount
    visibleTextUnchanged = $true
    svgQualityPassed = $true
    powerpointGeometryPassed = $true
    fallbackUsed = $false
    appliedToProject = [bool]$ApplyFix
    pptxPath = $pptxPath
}
$summaryPath = Join-Path $replayRoot "replay_summary.json"
Write-Utf8Atomic $summaryPath ($summary | ConvertTo-Json -Depth 10)
$summary | ConvertTo-Json -Depth 10
