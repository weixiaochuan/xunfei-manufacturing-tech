Set-StrictMode -Version Latest

function Assert-CloudPublicOrigin {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$Value,
        [Parameter(Mandatory = $true)]
        [string]$ExpectedHost
    )

    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "$Name must not be empty."
    }

    $uri = $null
    if (-not [Uri]::TryCreate($Value, [UriKind]::Absolute, [ref]$uri)) {
        throw "$Name must be a valid absolute URL."
    }

    $expectedOrigin = "https://$ExpectedHost"
    if (
        $Value -cne $expectedOrigin -or
        $uri.Scheme -ne [Uri]::UriSchemeHttps -or
        $uri.Host -cne $ExpectedHost -or
        -not $uri.IsDefaultPort -or
        $uri.AbsolutePath -ne "/" -or
        -not [string]::IsNullOrEmpty($uri.Query) -or
        -not [string]::IsNullOrEmpty($uri.Fragment) -or
        -not [string]::IsNullOrEmpty($uri.UserInfo)
    ) {
        throw "$Name must be exactly https://$ExpectedHost with no port, path, credentials, query, or fragment."
    }

    return $expectedOrigin
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
        throw "Cloud TEST artifacts must be written outside the Git repository."
    }
}
