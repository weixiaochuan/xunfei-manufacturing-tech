param([string]$InterfaceAlias)
. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
Assert-Administrator; $context=Get-PomegranateLanContext -InterfaceAlias $InterfaceAlias; Assert-PrivateLan $context
$subnet=Get-SubnetCidr $context.IPAddress $context.PrefixLength; $group='Pomegranate LAN Dev'
foreach($port in 3010,8000){$name="$group - TCP $port"; if(Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue){Remove-NetFirewallRule -DisplayName $name}; New-NetFirewallRule -DisplayName $name -DisplayGroup $group -Direction Inbound -Action Allow -Enabled True -Profile Private -Protocol TCP -LocalPort $port -RemoteAddress $subnet -InterfaceAlias $context.InterfaceAlias|Out-Null}
Write-Host "已仅为 Private 网络和子网 $subnet 开放 TCP 3010、8000；未开放 5432。"
