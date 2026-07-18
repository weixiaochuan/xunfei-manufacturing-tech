param(
    [Parameter(Mandatory = $true)]
    [string]$SourceProject,
    [string]$OutputPath = (Join-Path $PSScriptRoot "input_snapshot.json"),
    [string]$PptMasterRoot = "D:\大学\大二上各种作业\大三下\科大飞讯\xunfei-manufacturing-tech\7.9 第一周\Zhuhai\ppt-master",
    [string]$PythonPath = "D:\大学\大二上各种作业\大三下\科大飞讯\xunfei-manufacturing-tech\7.9 第一周\Zhuhai\ppt-master\.venv\Scripts\python.exe",
    [string]$OutputDir = "D:\大学\大二上各种作业\大三下\科大飞讯\xunfei-manufacturing-tech\7.9 第一周\Zhuhai\ppt-master\examples",
    [long]$MaterialSourceId = 7
)

$ErrorActionPreference = "Stop"

function Read-Utf8([string]$Path) {
    return [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
}

function Match-Group([string]$Text, [string]$Pattern, [string]$Name) {
    $match = [System.Text.RegularExpressions.Regex]::Match($Text, $Pattern)
    if (-not $match.Success) {
        throw "Cannot recover '$Name' from persisted planning context."
    }
    return $match.Groups[$Name].Value.Trim()
}

function Markdown-Section([string]$Planning, [string]$Title) {
    $escaped = [System.Text.RegularExpressions.Regex]::Escape($Title)
    return Match-Group $Planning "(?ms)^## $escaped\r?\n(?<value>.*?)(?=^## |\z)" "value"
}

$planningPath = Join-Path $SourceProject "sources\planning_context.md"
$promptPath = Join-Path $SourceProject "sources\confirmed_prompt.md"
$statePath = Join-Path $SourceProject "native_generation_state.json"
$planPath = Join-Path $SourceProject "slide_plan.json"
foreach ($required in @($planningPath, $promptPath, $statePath, $planPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required persisted input artifact is missing: $required"
    }
}

$persisted = Read-Utf8 $planningPath
$prompt = (Read-Utf8 $promptPath).Trim()
$state = (Read-Utf8 $statePath) | ConvertFrom-Json
$plan = (Read-Utf8 $planPath) | ConvertFrom-Json

$planningContext = Match-Group $persisted "(?ms)^\[User-Edited Structured AI Understanding\]\r?\n(?<planning>.*?)(?=\r?\n\r?\n\[Audience\])" "planning"
$audience = Match-Group $persisted "(?ms)^\[Audience\]\r?\n(?<audience>.*?)(?=\r?\n\r?\n\[Raw Material)" "audience"
$rawMaterial = Match-Group $persisted "(?ms)^\[Raw Material[^\]]*\]\r?\n(?<material>.*?)(?=\r?\n\r?\n\[Extra Requirements\])" "material"
$extraRequirements = Match-Group $persisted "(?ms)^\[Extra Requirements\]\r?\n(?<extra>.*?)(?=\r?\n\r?\n\[Legacy Prompt)" "extra"

$understanding = [ordered]@{
    understandingSummary = Markdown-Section $planningContext "AI 理解摘要"
    keyPriorities = Markdown-Section $planningContext "重点取舍"
    suggestedPageStructure = Markdown-Section $planningContext "建议页面结构"
    narrativeMainline = Markdown-Section $planningContext "叙事主线"
    visualExpressionAdvice = Markdown-Section $planningContext "视觉与表达建议"
    openQuestions = Markdown-Section $planningContext "仍需确认的问题"
}

$style = [string]$plan.style
if ($style -ne "科技蓝") {
    throw "This capture script currently only freezes the verified current style preset '科技蓝'; actual style was '$style'."
}

$payload = [ordered]@{
    pptMasterRoot = $PptMasterRoot
    pythonPath = $PythonPath
    prompt = $prompt
    planningContext = $planningContext
    aiUnderstandingResult = $understanding
    understandingSummary = $understanding.understandingSummary
    keyPriorities = $understanding.keyPriorities
    suggestedPageStructure = $understanding.suggestedPageStructure
    narrativeMainline = $understanding.narrativeMainline
    visualExpressionAdvice = $understanding.visualExpressionAdvice
    openQuestions = $understanding.openQuestions
    rawMaterial = $rawMaterial
    materialSources = @(
        [ordered]@{
            id = $MaterialSourceId
            sourceType = "document"
            title = "毛泽东 素材"
        }
    )
    extraRequirements = $extraRequirements
    modelId = [long]$state.model.databaseId
    title = $prompt
    audience = $audience
    slideCount = [int]$state.slideCount
    style = $style
    generationEngine = "ppt_master_native"
    mode = "showcase"
    visualStyle = "dark-tech"
    layoutBias = @("ai_ops")
    chartBias = @("pipeline_with_stages", "process_flow", "layered_architecture", "kpi_cards")
    outputDir = $OutputDir
    generationMode = "agent"
}

$snapshot = [ordered]@{
    schemaVersion = 1
    capturedAt = [DateTimeOffset]::UtcNow.ToString("o")
    inputSource = "persisted output of the current Pomegranate confirmation payload"
    sourceProjectPath = [System.IO.Path]::GetFullPath($SourceProject)
    sourceInputFingerprint = [string]$state.inputFingerprint
    modelMetadata = [ordered]@{
        databaseId = [long]$state.model.databaseId
        provider = [string]$state.model.provider
        modelId = [string]$state.model.modelId
    }
    payload = $payload
}

$parent = Split-Path -Parent $OutputPath
if ($parent) {
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
}
$json = $snapshot | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($OutputPath, $json, [System.Text.UTF8Encoding]::new($false))
Write-Output "snapshot=$OutputPath"
Write-Output "sourceFingerprint=$($state.inputFingerprint)"
Write-Output "rawMaterialCharacters=$($rawMaterial.Length)"
Write-Output "modelDatabaseId=$($state.model.databaseId)"
