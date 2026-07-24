$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'CloudClient.Common.ps1')

function Assert-Rejected {
    param(
        [string]$Name,
        [string]$Value,
        [string]$ExpectedHost
    )

    try {
        Assert-CloudPublicOrigin -Name $Name -Value $Value -ExpectedHost $ExpectedHost | Out-Null
    } catch {
        return
    }
    throw "An unsafe URL was accepted for $Name."
}

$api = Assert-CloudPublicOrigin `
    -Name 'ApiBaseUrl' `
    -Value 'https://api.stargathering.cn' `
    -ExpectedHost 'api.stargathering.cn'
$auth = Assert-CloudPublicOrigin `
    -Name 'AuthBaseUrl' `
    -Value 'https://auth.stargathering.cn' `
    -ExpectedHost 'auth.stargathering.cn'

if ($api -ne 'https://api.stargathering.cn') {
    throw 'The normalized API URL is incorrect.'
}
if ($auth -ne 'https://auth.stargathering.cn') {
    throw 'The normalized auth URL is incorrect.'
}

foreach ($value in @(
    '',
    'http://api.stargathering.cn',
    'http://127.0.0.1:3010',
    'http://localhost:3010',
    'http://192.168.31.210:3010',
    'https://localhost:3010',
    'https://127.0.0.1:3010',
    'https://api.example.com',
    'https://user:password@api.stargathering.cn',
    'ftp://api.stargathering.cn',
    'https://api.stargathering.cn:443',
    'https://api.stargathering.cn/v1',
    'https://api.stargathering.cn?unsafe=true'
)) {
    Assert-Rejected -Name 'ApiBaseUrl' -Value $value -ExpectedHost 'api.stargathering.cn'
}

foreach ($value in @(
    '',
    'http://auth.stargathering.cn',
    'https://localhost:8000',
    'https://auth.example.com',
    'https://auth.stargathering.cn/oauth'
)) {
    Assert-Rejected -Name 'AuthBaseUrl' -Value $value -ExpectedHost 'auth.stargathering.cn'
}

Write-Host 'Cloud TEST public URL tests passed.'
