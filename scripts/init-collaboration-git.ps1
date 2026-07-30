param(
  [string]$BranchName = "baseline/ag-collaboration",
  [string]$CommitMessage = "chore: establish AG collaboration baseline",
  [string]$AuthorName = "AG Collaboration",
  [string]$AuthorEmail = "ag-collaboration@example.local",
  [string]$CommitDate = "2026-07-31T00:00:00+08:00"
)

$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

if (Test-Path -LiteralPath ".git") {
  $inside = git rev-parse --is-inside-work-tree 2>$null
  if ($LASTEXITCODE -eq 0 -and $inside -eq "true") {
    $head = git rev-parse --verify HEAD 2>$null
    if ($LASTEXITCODE -eq 0) {
      Write-Host "Git repository already has a HEAD commit. No baseline initialization needed."
      exit 0
    }
  }
} else {
  git init
}

git switch -c $BranchName 2>$null
if ($LASTEXITCODE -ne 0) {
  git checkout -b $BranchName
}

$paths = @(
  ".docs-meta.json",
  ".gitignore",
  ".npmrc",
  "AGENTS.md",
  "AG多人协作与功能迁移说明.md",
  "CLAUDE.md",
  "COMMERCIAL-LICENSE.md",
  "CONTRIBUTING.md",
  "INSTALL.md",
  "LICENSE",
  "cc.bat",
  "index.html",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "rust-toolchain.toml",
  "status.json",
  "tsconfig.json",
  "tsconfig.node.json",
  "vite.config.ts",
  "archive",
  "dev-plugins",
  "docs",
  "plugins",
  "prototypes",
  "public",
  "scripts",
  "services",
  "src",
  "src-tauri",
  "tasks",
  "templates",
  "tools"
)

foreach ($path in $paths) {
  if (Test-Path -LiteralPath $path) {
    git add -- $path
  }
}

$env:GIT_AUTHOR_NAME = $AuthorName
$env:GIT_AUTHOR_EMAIL = $AuthorEmail
$env:GIT_AUTHOR_DATE = $CommitDate
$env:GIT_COMMITTER_NAME = $AuthorName
$env:GIT_COMMITTER_EMAIL = $AuthorEmail
$env:GIT_COMMITTER_DATE = $CommitDate

git commit -m $CommitMessage
Write-Host "Baseline commit created on branch $BranchName."
