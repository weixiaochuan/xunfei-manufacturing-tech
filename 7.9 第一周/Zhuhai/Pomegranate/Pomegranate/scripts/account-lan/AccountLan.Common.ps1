Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-RepositoryRoot { return (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path }

function Get-PomegranateLanContext {
    param([string]$InterfaceAlias)
    $virtualPattern = 'Docker|WSL|Hyper-V|VMware|VirtualBox|VPN|Radmin|TAP|TUN|Loopback|vEthernet'
    $items = foreach ($cfg in Get-NetIPConfiguration) {
        if (-not $cfg.IPv4DefaultGateway) { continue }
        $adapter = Get-NetAdapter -InterfaceIndex $cfg.InterfaceIndex -ErrorAction SilentlyContinue
        if (-not $adapter -or $adapter.Status -ne 'Up' -or -not $adapter.HardwareInterface) { continue }
        if (($adapter.Name + ' ' + $adapter.InterfaceDescription) -match $virtualPattern) { continue }
        foreach ($address in $cfg.IPv4Address) {
            if ($address.IPAddress -eq '127.0.0.1' -or $address.IPAddress -like '169.254.*') { continue }
            $profile = Get-NetConnectionProfile -InterfaceIndex $cfg.InterfaceIndex -ErrorAction Stop
            [pscustomobject]@{ InterfaceAlias=$adapter.Name; InterfaceIndex=$cfg.InterfaceIndex; IPAddress=$address.IPAddress; PrefixLength=$address.PrefixLength; NetworkCategory=[string]$profile.NetworkCategory }
        }
    }
    if ($InterfaceAlias) { $items = @($items | Where-Object InterfaceAlias -eq $InterfaceAlias) }
    if (@($items).Count -eq 0) { throw '没有找到带默认网关的活动物理 Wi-Fi/以太网 IPv4。' }
    if (@($items).Count -gt 1) { $items | Format-Table InterfaceAlias,IPAddress,PrefixLength,NetworkCategory; throw '检测到多个候选网卡，请使用 -InterfaceAlias 明确选择。' }
    return @($items)[0]
}

function Assert-PrivateLan { param($Context); if ($Context.NetworkCategory -ne 'Private') { throw "网络 '$($Context.InterfaceAlias)' 当前是 $($Context.NetworkCategory)。请先在 Windows 设置中改为专用网络（Private），再重试。" } }

function Get-SubnetCidr {
    param([string]$IPAddress,[int]$PrefixLength)
    $bytes=[Net.IPAddress]::Parse($IPAddress).GetAddressBytes(); $remaining=$PrefixLength
    for($i=0;$i -lt 4;$i++){ $bits=[Math]::Min(8,[Math]::Max(0,$remaining)); $mask=if($bits -eq 0){0}else{(0xFF -shl (8-$bits))-band 0xFF}; $bytes[$i]=$bytes[$i]-band $mask; $remaining-=$bits }
    return "$([Net.IPAddress]::new($bytes))/$PrefixLength"
}

function Import-SafeEnvFile {
    param([string]$Path)
    if(-not(Test-Path -LiteralPath $Path)){throw "缺少环境文件：$Path"}
    foreach($line in Get-Content -LiteralPath $Path){ if($line -match '^\s*#' -or $line -match '^\s*$'){continue}; if($line -notmatch '^([A-Za-z_][A-Za-z0-9_]*)=(.*)$'){throw "环境文件含无效行（未输出内容）：$Path"}; [Environment]::SetEnvironmentVariable($Matches[1],$Matches[2],'Process') }
}

function Get-ComposeLanArgs { $root=Get-RepositoryRoot; return @('--env-file',(Join-Path $root '.env.account'),'--env-file',(Join-Path $root '.env.account.lan'),'-f',(Join-Path $root 'compose.account.yml'),'-f',(Join-Path $root 'compose.account.lan.yml')) }

function Assert-Administrator { $identity=[Security.Principal.WindowsIdentity]::GetCurrent(); $principal=[Security.Principal.WindowsPrincipal]::new($identity); if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw '此操作需要管理员权限，请以管理员身份打开 PowerShell。'} }
