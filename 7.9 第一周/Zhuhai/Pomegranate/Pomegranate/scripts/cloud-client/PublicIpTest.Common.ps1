Set-StrictMode -Version Latest

function Test-PublicIpv4Address {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Address
    )

    $parsedAddress = $null
    if (
        -not [Net.IPAddress]::TryParse($Address, [ref]$parsedAddress) -or
        $parsedAddress.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork
    ) {
        return $false
    }

    $octets = $parsedAddress.GetAddressBytes()
    $first = [int]$octets[0]
    $second = [int]$octets[1]
    $third = [int]$octets[2]
    $fourth = [int]$octets[3]
    if ("$first.$second.$third.$fourth" -cne $Address) {
        return $false
    }

    return -not (
        $first -eq 0 -or
        $first -eq 10 -or
        $first -eq 127 -or
        ($first -eq 100 -and $second -ge 64 -and $second -le 127) -or
        ($first -eq 169 -and $second -eq 254) -or
        ($first -eq 172 -and $second -ge 16 -and $second -le 31) -or
        ($first -eq 192 -and $second -eq 0 -and $third -eq 0) -or
        ($first -eq 192 -and $second -eq 0 -and $third -eq 2) -or
        ($first -eq 192 -and $second -eq 88 -and $third -eq 99) -or
        ($first -eq 192 -and $second -eq 168) -or
        ($first -eq 198 -and ($second -eq 18 -or $second -eq 19)) -or
        ($first -eq 198 -and $second -eq 51 -and $third -eq 100) -or
        ($first -eq 203 -and $second -eq 0 -and $third -eq 113) -or
        $first -ge 224 -or
        ($first -eq 255 -and $second -eq 255 -and $third -eq 255 -and $fourth -eq 255)
    )
}

function Assert-PublicIpTestOrigin {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [switch]$AllowInsecureHttp
    )

    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "$Name must not be empty."
    }

    $match = [regex]::Match(
        $Value,
        '^(?<scheme>https?)://(?<host>(?:[0-9]{1,3}\.){3}[0-9]{1,3}):(?<port>[0-9]{1,5})/?$',
        [Text.RegularExpressions.RegexOptions]::CultureInvariant
    )
    if (-not $match.Success) {
        throw "$Name must contain only HTTP(S), a public IPv4 address, and an explicit port."
    }

    $scheme = $match.Groups['scheme'].Value
    $hostName = $match.Groups['host'].Value
    $port = [int]$match.Groups['port'].Value
    if ($port -lt 1 -or $port -gt 65535) {
        throw "$Name port must be between 1 and 65535."
    }
    if (-not (Test-PublicIpv4Address -Address $hostName)) {
        throw "$Name must use a globally routable public IPv4 address."
    }
    if ($scheme -eq 'http' -and -not $AllowInsecureHttp) {
        throw "$Name uses HTTP. Pass -AllowInsecureHttp only for temporary testing."
    }

    return "${scheme}://${hostName}:${port}"
}

function Assert-PublicIpTestTauriConfig {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ConfigPath
    )

    $config = Get-Content -LiteralPath $ConfigPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($config.productName -ne 'Pomegranate Public IP TEST') {
        throw 'Public IP TEST productName is incorrect.'
    }
    if ($config.mainBinaryName -ne 'Pomegranate-Public-IP-TEST') {
        throw 'Public IP TEST mainBinaryName is incorrect.'
    }
    if (
        $config.plugins.'deep-link'.desktop.schemes.Count -ne 1 -or
        $config.plugins.'deep-link'.desktop.schemes[0] -ne 'pomegranate'
    ) {
        throw 'Public IP TEST deep-link configuration is incorrect.'
    }
    if ($config.plugins.updater.endpoints.Count -ne 0) {
        throw 'Public IP TEST updater endpoints must be empty.'
    }
    if ($config.bundle.createUpdaterArtifacts -ne $false) {
        throw 'Public IP TEST updater artifacts must be disabled.'
    }
    if ($config.bundle.targets.Count -ne 1 -or $config.bundle.targets[0] -ne 'nsis') {
        throw 'Public IP TEST must use the NSIS bundle target.'
    }

    $rawConfig = Get-Content -LiteralPath $ConfigPath -Raw -Encoding UTF8
    $configWithoutSchema = $rawConfig.Replace(
        'https://schema.tauri.app/config/2',
        ''
    )
    if ($configWithoutSchema -match '(?i)https?://|client.?secret|password|database|user.files') {
        throw 'Public IP TEST Tauri config contains a URL or sensitive server setting.'
    }
}

function Assert-OutputOutsideRepository {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot,
        [Parameter(Mandatory = $true)]
        [string]$OutputDirectory
    )

    $repository = [IO.Path]::GetFullPath($RepositoryRoot).TrimEnd('\') + '\'
    $output = [IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\') + '\'
    if ($output.StartsWith($repository, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Public IP TEST artifacts must be written outside the Git repository.'
    }
}
