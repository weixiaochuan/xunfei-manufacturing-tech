$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'PublicIpTest.Common.ps1')

function Assert-Rejected {
    param(
        [string]$Name,
        [string]$Value,
        [switch]$AllowInsecureHttp
    )

    try {
        Assert-PublicIpTestOrigin `
            -Name $Name `
            -Value $Value `
            -AllowInsecureHttp:$AllowInsecureHttp | Out-Null
    } catch {
        return
    }
    throw "An unsafe Public IP TEST URL was accepted for $Name."
}

function Get-PublicIpv4TestFixture {
    foreach ($first in 1..223) {
        $candidate = "$first.1.1.1"
        if (Test-PublicIpv4Address -Address $candidate) {
            return $candidate
        }
    }
    throw 'No syntactically public IPv4 test fixture was found.'
}

$publicIp = Get-PublicIpv4TestFixture
$httpsPort = 49152
$httpPort = 49153
$httpsInput = "https://${publicIp}:${httpsPort}"
$httpInput = "http://${publicIp}:${httpPort}"

$httpsOrigin = Assert-PublicIpTestOrigin `
    -Name 'ApiBaseUrl' `
    -Value $httpsInput
if ($httpsOrigin -ne $httpsInput) {
    throw 'HTTPS Public IP TEST origin normalization failed.'
}

Assert-Rejected -Name 'ApiBaseUrl' -Value $httpInput
$httpOrigin = Assert-PublicIpTestOrigin `
    -Name 'ApiBaseUrl' `
    -Value $httpInput `
    -AllowInsecureHttp
if ($httpOrigin -ne $httpInput) {
    throw 'Temporary HTTP Public IP TEST origin normalization failed.'
}

foreach ($value in @(
    '',
    "https://${publicIp}",
    'https://localhost:8443',
    'https://127.0.0.1:8443',
    'https://0.0.0.0:8443',
    'https://10.0.0.8:8443',
    'https://172.16.0.8:8443',
    'https://192.168.1.8:8443',
    'https://169.254.1.8:8443',
    'https://192.0.2.8:8443',
    'https://198.51.100.8:8443',
    'https://203.0.113.8:8443',
    'https://255.255.255.255:8443',
    "ftp://${publicIp}:${httpsPort}",
    "file://${publicIp}:${httpsPort}",
    "https://user:password@${publicIp}:${httpsPort}",
    "https://${publicIp}:${httpsPort}/v1",
    "https://${publicIp}:${httpsPort}?query=value",
    "https://${publicIp}:${httpsPort}#fragment"
)) {
    Assert-Rejected -Name 'ApiBaseUrl' -Value $value -AllowInsecureHttp
}

Write-Host 'Public IP TEST URL tests passed.'
