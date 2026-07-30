param(
  [string]$ProjectRoot = (Resolve-Path "$PSScriptRoot\..\..").Path,
  [string]$RuntimeRoot = "D:\pomegranate-local-test",
  [string]$PostgresRoot = "E:\ag-tools\pgsql"
)

. "$PSScriptRoot\common.ps1"
Initialize-AccountTestPaths -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
Assert-AccountTestPrerequisites -RequireCasdoorSecrets
Set-AccountServerTestEnvironment

function Invoke-AllowHttpError {
  param([Parameter(Mandatory=$true)][string]$Uri)
  $request = [System.Net.HttpWebRequest]::Create($Uri)
  $request.AllowAutoRedirect = $false
  try {
    return $request.GetResponse()
  } catch [System.Net.WebException] {
    if ($_.Exception.Response) {
      return $_.Exception.Response
    }
    throw
  } catch {
    $responseProperty = $_.Exception.PSObject.Properties["Response"]
    if ($responseProperty -and $responseProperty.Value) {
      return $responseProperty.Value
    }
    throw
  }
}

if (!(Test-TcpPort -HostName "127.0.0.1" -Port 55432)) { throw "PostgreSQL is not ready on 127.0.0.1:55432" }

$live = Invoke-WebRequest -Uri "http://127.0.0.1:18080/health/live" -UseBasicParsing
if ($live.StatusCode -ne 200) { throw "/health/live did not return 200" }
$ready = Invoke-WebRequest -Uri "http://127.0.0.1:18080/health/ready" -UseBasicParsing
if ($ready.StatusCode -ne 200) { throw "/health/ready did not return 200" }
$login = Invoke-AllowHttpError "http://127.0.0.1:18080/auth/login?client=desktop"
if ($login.StatusCode -ne 302) { throw "/auth/login?client=desktop did not return 302" }
try {
  $discovery = Invoke-WebRequest -Uri "http://82.157.119.201:18000/.well-known/openid-configuration" -UseBasicParsing -TimeoutSec 10
} catch {
  throw "Casdoor is not reachable at http://82.157.119.201:18000/.well-known/openid-configuration. Confirm the remote Casdoor service is running and port 18000 is open."
}
if ($discovery.StatusCode -ne 200) { throw "Casdoor discovery did not return 200" }

foreach ($path in @("/files", "/documents")) {
  $response = Invoke-AllowHttpError "http://127.0.0.1:18080$path"
  if ($response.StatusCode -eq 404) { throw "$path returned route 404 instead of an auth error" }
}

Write-Host "Account TEST runtime checks passed."
