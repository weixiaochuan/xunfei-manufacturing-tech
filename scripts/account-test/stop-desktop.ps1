param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

Get-Process intelligent_notebook,tauri,vite,node -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -like "$ProjectRoot*" -or $_.ProcessName -eq "vite" } |
  Stop-Process -Force
