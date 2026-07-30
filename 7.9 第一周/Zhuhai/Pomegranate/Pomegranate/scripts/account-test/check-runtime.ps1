param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
$result = [ordered]@{
    postgresDataInitialized = Test-Path -LiteralPath (Join-Path $context.PostgresData 'PG_VERSION') -PathType Leaf
    casdoorClientIdFile = (Test-Path -LiteralPath $context.CasdoorClientIdFile -PathType Leaf) -and ((Get-Item $context.CasdoorClientIdFile).Length -gt 0)
    casdoorClientSecretFile = (Test-Path -LiteralPath $context.CasdoorClientSecretFile -PathType Leaf) -and ((Get-Item $context.CasdoorClientSecretFile).Length -gt 0)
    postgresListening = [bool](Get-NetTCPConnection -LocalAddress '127.0.0.1' -LocalPort 55432 -State Listen -ErrorAction SilentlyContinue)
    postgresPubliclyListening = [bool](Get-NetTCPConnection -LocalPort 55432 -State Listen -ErrorAction SilentlyContinue | Where-Object LocalAddress -NotIn @('127.0.0.1','::1'))
    accountServerListening = [bool](Get-NetTCPConnection -LocalAddress '127.0.0.1' -LocalPort 18080 -State Listen -ErrorAction SilentlyContinue)
    casdoorDiscovery = $null
    healthLive = $null
    healthReady = $null
    sessionUnauthenticated = $null
    filesUnauthenticated = $null
}
foreach ($probe in @(
    @{ Name = 'casdoorDiscovery'; Uri = "$($context.CasdoorPublicUrl)/.well-known/openid-configuration" },
    @{ Name = 'healthLive'; Uri = "$($context.AccountServerPublicUrl)/health/live" },
    @{ Name = 'healthReady'; Uri = "$($context.AccountServerPublicUrl)/health/ready" },
    @{ Name = 'sessionUnauthenticated'; Uri = "$($context.AccountServerPublicUrl)/auth/session" },
    @{ Name = 'filesUnauthenticated'; Uri = "$($context.AccountServerPublicUrl)/files" }
)) {
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri $probe.Uri -TimeoutSec 5
        $result[$probe.Name] = [int]$response.StatusCode
    } catch {
        if ($_.Exception.Response) { $result[$probe.Name] = [int]$_.Exception.Response.StatusCode }
    }
}
[pscustomobject]$result | ConvertTo-Json
