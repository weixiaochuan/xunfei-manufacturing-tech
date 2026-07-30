param([string] $ProjectRoot, [string] $RuntimeRoot, [string] $PostgresRoot)
. (Join-Path $PSScriptRoot 'common.ps1')
$context = Resolve-AccountTestContext -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
New-Item -ItemType Directory -Force -Path $context.RuntimeRoot, (Join-Path $context.RuntimeRoot 'logs'), $context.UserFilesRoot | Out-Null

if (-not (Test-Path -LiteralPath $context.PostgresPasswordFile -PathType Leaf)) {
    $bytes = New-Object byte[] 48
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }
    $password = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
    [System.IO.File]::WriteAllText($context.PostgresPasswordFile, $password, [System.Text.UTF8Encoding]::new($false))
    icacls.exe $context.PostgresPasswordFile /inheritance:r /grant:r "$env:USERNAME`:F" | Out-Null
    $password = $null
}

if (-not (Test-Path -LiteralPath (Join-Path $context.PostgresData 'PG_VERSION') -PathType Leaf)) {
    New-Item -ItemType Directory -Force -Path $context.PostgresData | Out-Null
    $passwordArgument = '--pwfile={0}' -f $context.PostgresPasswordFile
    & (Join-Path $context.PostgresBin 'initdb.exe') -D $context.PostgresData -U $context.PostgresUser --auth-host=scram-sha-256 --auth-local=scram-sha-256 $passwordArgument --encoding=UTF8 --locale=C
    Assert-AccountTestCommand 'PostgreSQL TEST data initialization'
    [System.IO.File]::AppendAllText(
        (Join-Path $context.PostgresData 'postgresql.conf'),
        "`r`n# Pomegranate isolated account TEST runtime`r`nlisten_addresses = '127.0.0.1'`r`nport = 55432`r`npassword_encryption = 'scram-sha-256'`r`n",
        [System.Text.UTF8Encoding]::new($false)
    )
    [System.IO.File]::WriteAllText(
        (Join-Path $context.PostgresData 'pg_hba.conf'),
        "# Pomegranate isolated account TEST runtime`r`nhost all all 127.0.0.1/32 scram-sha-256`r`nhost all all ::1/128 reject`r`n",
        [System.Text.UTF8Encoding]::new($false)
    )
}
Write-Output 'PostgreSQL TEST data directory is initialized.'
