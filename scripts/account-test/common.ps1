param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-AccountTestPath {
  param([Parameter(Mandatory=$true)][string]$Path)
  return [System.IO.Path]::GetFullPath($Path)
}

function Initialize-AccountTestPaths {
  param(
    [Parameter(Mandatory=$true)][string]$ProjectRoot,
    [Parameter(Mandatory=$true)][string]$RuntimeRoot,
    [Parameter(Mandatory=$true)][string]$PostgresRoot
  )

  $script:ProjectRootPath = Resolve-AccountTestPath $ProjectRoot
  $script:RuntimeRootPath = Resolve-AccountTestPath $RuntimeRoot
  $script:PostgresRootPath = Resolve-AccountTestPath $PostgresRoot
  $script:ToolRootPath = Resolve-AccountTestPath "E:\ag-tools"
  $script:AccountServerRoot = Join-Path $script:ProjectRootPath "services\account-server"
  $script:LogRoot = Join-Path $script:RuntimeRootPath "logs"
  $script:PostgresDataRoot = Join-Path $script:RuntimeRootPath "postgres-data"
  $script:UserFilesRoot = Join-Path $script:RuntimeRootPath "user-files"
  $script:DesktopDataRoot = Join-Path $script:RuntimeRootPath "desktop-data"

  $preferredPathEntries = @(
    (Join-Path $script:ToolRootPath "node-v22.23.0-win-x64"),
    (Join-Path $script:ToolRootPath "npm-global")
  ) | Where-Object { Test-Path -LiteralPath $_ }
  if ($preferredPathEntries.Count -gt 0) {
    $env:PATH = (($preferredPathEntries + @($env:PATH)) -join [System.IO.Path]::PathSeparator)
  }
  $env:PNPM_HOME = Join-Path $script:ToolRootPath "npm-global"
  $env:PNPM_STORE_DIR = Join-Path $script:ToolRootPath "pnpm-store"
  $env:npm_config_store_dir = $env:PNPM_STORE_DIR
}

function Ensure-AccountTestDirectories {
  foreach ($path in @($script:RuntimeRootPath, $script:LogRoot, $script:UserFilesRoot, $script:DesktopDataRoot)) {
    New-Item -ItemType Directory -Force -Path $path | Out-Null
  }
}

function Read-SecretFile {
  param([Parameter(Mandatory=$true)][string]$Name)
  $path = Join-Path $script:RuntimeRootPath $Name
  if (!(Test-Path -LiteralPath $path)) {
    throw "Missing required local secret file: $path"
  }
  $value = (Get-Content -LiteralPath $path -Raw).Trim()
  if ([string]::IsNullOrWhiteSpace($value)) {
    throw "Local secret file is empty: $path"
  }
  return $value
}

function Get-PostgresBin {
  param([Parameter(Mandatory=$true)][string]$ExeName)
  $path = Join-Path $script:PostgresRootPath "bin\$ExeName"
  if (!(Test-Path -LiteralPath $path)) {
    throw "PostgreSQL tool not found: $path"
  }
  return $path
}

function Get-RequiredCommand {
  param([Parameter(Mandatory=$true)][string]$Name)
  $command = Get-Command $Name -ErrorAction SilentlyContinue
  if (!$command) {
    throw "Required command not found on PATH: $Name"
  }
  return $command.Source
}

function Import-MsvcBuildEnvironment {
  if (Get-Command "link.exe" -ErrorAction SilentlyContinue) {
    Use-ExternalWindowsSdk
    return
  }

  $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (!(Test-Path -LiteralPath $vswhere)) {
    throw "MSVC linker not found. Install Visual Studio Build Tools with Desktop development with C++."
  }

  $installationPath = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
  if ([string]::IsNullOrWhiteSpace($installationPath)) {
    throw "MSVC C++ build tools not found. Install Visual Studio Build Tools with Desktop development with C++."
  }

  $vsDevCmd = Join-Path $installationPath "Common7\Tools\VsDevCmd.bat"
  if (!(Test-Path -LiteralPath $vsDevCmd)) {
    throw "Visual Studio developer command script not found: $vsDevCmd"
  }

  cmd.exe /s /c "`"$vsDevCmd`" -arch=x64 -host_arch=x64 >nul && set" |
    ForEach-Object {
      $separator = $_.IndexOf("=")
      if ($separator -gt 0) {
        $name = $_.Substring(0, $separator)
        $value = $_.Substring($separator + 1)
        [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
      }
    }

  Use-ExternalWindowsSdk
}

function Use-ExternalWindowsSdk {
  $sdkRoot = Join-Path $script:ToolRootPath "Windows Kits\10"
  if (!(Test-Path -LiteralPath $sdkRoot)) { return }

  $libRoot = Join-Path $sdkRoot "Lib"
  $sdkVersion = Get-ChildItem -LiteralPath $libRoot -Directory -ErrorAction SilentlyContinue |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "um\x64\kernel32.Lib") } |
    Sort-Object Name -Descending |
    Select-Object -First 1 -ExpandProperty Name
  if ([string]::IsNullOrWhiteSpace($sdkVersion)) { return }

  $cSdkRoot = "C:\Program Files (x86)\Windows Kits\10"
  $replacementNames = @("PATH", "INCLUDE", "LIB", "LIBPATH")
  foreach ($name in $replacementNames) {
    $value = [System.Environment]::GetEnvironmentVariable($name, "Process")
    if ($value) {
      [System.Environment]::SetEnvironmentVariable($name, $value.Replace($cSdkRoot, $sdkRoot), "Process")
    }
  }

  $sdkBin = Join-Path $sdkRoot "bin\$sdkVersion\x64"
  $sdkInclude = Join-Path $sdkRoot "Include\$sdkVersion"
  $sdkLib = Join-Path $sdkRoot "Lib\$sdkVersion"
  $unionMetadata = Join-Path $sdkRoot "UnionMetadata\$sdkVersion"
  $references = Join-Path $sdkRoot "References\$sdkVersion"

  $env:WindowsSdkDir = "$sdkRoot\"
  $env:WindowsSDKLibVersion = "$sdkVersion\"
  $env:WindowsSDKVersion = "$sdkVersion\"
  $env:UniversalCRTSdkDir = "$sdkRoot\"
  $env:UCRTVersion = "$sdkVersion\"
  $env:PATH = "$sdkBin;$env:PATH"
  $env:INCLUDE = "$(Join-Path $sdkInclude "ucrt");$(Join-Path $sdkInclude "um");$(Join-Path $sdkInclude "shared");$(Join-Path $sdkInclude "winrt");$(Join-Path $sdkInclude "cppwinrt");$env:INCLUDE"
  $env:LIB = "$(Join-Path $sdkLib "ucrt\x64");$(Join-Path $sdkLib "um\x64");$env:LIB"
  $env:LIBPATH = "$unionMetadata;$references;$env:LIBPATH"
}

function Assert-AccountTestPrerequisites {
  param(
    [switch]$RequireCasdoorSecrets,
    [switch]$RequirePnpm,
    [switch]$RequirePostgresTools
  )

  Get-RequiredCommand "node.exe" | Out-Null
  if ($RequirePnpm) {
    Get-RequiredCommand "pnpm.cmd" | Out-Null
  }
  if ($RequirePostgresTools) {
    Get-PostgresBin "createdb.exe" | Out-Null
    Get-PostgresBin "initdb.exe" | Out-Null
    Get-PostgresBin "pg_ctl.exe" | Out-Null
    Get-PostgresBin "psql.exe" | Out-Null
  }

  if ($RequireCasdoorSecrets) {
    Read-SecretFile "casdoor-client-id.tmp" | Out-Null
    Read-SecretFile "casdoor-client-secret.tmp" | Out-Null
  }
}

function Set-AccountServerTestEnvironment {
  $env:NODE_ENV = "development"
  $env:DEPLOYMENT_PROFILE = "local"
  $env:ALLOW_LOCAL_TEST_CASDOOR = "true"
  $env:ALLOW_INSECURE_PUBLIC_IP_TEST = "false"
  $env:ACCOUNT_SERVER_HOST = "127.0.0.1"
  $env:ACCOUNT_SERVER_PORT = "18080"
  $env:ACCOUNT_SERVER_PUBLIC_URL = "http://127.0.0.1:18080"
  $env:ACCOUNT_DB_HOST = "127.0.0.1"
  $env:ACCOUNT_DB_PORT = "55432"
  $env:ACCOUNT_DB_NAME = "pomegranate_account_test"
  $env:ACCOUNT_DB_USER = "pomegranate_account_test"
  $env:ACCOUNT_DB_PASSWORD = Read-SecretFile "postgres-password.tmp"
  $env:CASDOOR_BASE_URL = "http://82.157.119.201:18000"
  $env:CASDOOR_PUBLIC_URL = "http://82.157.119.201:18000"
  $env:CASDOOR_REDIRECT_URI = "http://127.0.0.1:18080/auth/callback"
  $env:CASDOOR_ORGANIZATION = "pomegranate-test"
  $env:CASDOOR_APPLICATION = "app-pomegranate-test"
  $env:CASDOOR_CLIENT_ID = Read-SecretFile "casdoor-client-id.tmp"
  $env:CASDOOR_CLIENT_SECRET = Read-SecretFile "casdoor-client-secret.tmp"
  $env:FILE_STORAGE_BACKEND = "filesystem"
  $env:USER_FILES_ROOT = $script:UserFilesRoot
  $env:USER_FILE_MAX_BYTES = "104857600"
}

function Test-TcpPort {
  param([string]$HostName, [int]$Port)
  $client = [System.Net.Sockets.TcpClient]::new()
  try {
    $task = $client.ConnectAsync($HostName, $Port)
    return $task.Wait(1000) -and $client.Connected
  } finally {
    $client.Dispose()
  }
}

function Stop-PidFileProcess {
  param([Parameter(Mandatory=$true)][string]$PidFile)
  if (!(Test-Path -LiteralPath $PidFile)) { return }
  $pidValue = (Get-Content -LiteralPath $PidFile -Raw).Trim()
  if ($pidValue -match '^\d+$') {
    $process = Get-Process -Id ([int]$pidValue) -ErrorAction SilentlyContinue
    if ($process) {
      Stop-Process -Id $process.Id -Force
      Wait-Process -Id $process.Id -Timeout 10 -ErrorAction SilentlyContinue
    }
  }
  Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
}
