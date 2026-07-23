param([string]$InterfaceAlias,[switch]$Force)
. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
$context=Get-PomegranateLanContext -InterfaceAlias $InterfaceAlias; Assert-PrivateLan $context
$target=Join-Path (Get-RepositoryRoot) '.env.account.lan'
if((Test-Path -LiteralPath $target)-and-not $Force){throw '.env.account.lan 已存在；如确认需要重建，请使用 -Force。'}
$ip=$context.IPAddress
$content=@('DEPLOYMENT_PROFILE=lan',"LAN_IP=$ip",'ACCOUNT_SERVER_HOST=0.0.0.0','ACCOUNT_SERVER_PORT=3010',"ACCOUNT_SERVER_PUBLIC_URL=http://${ip}:3010", "CASDOOR_PUBLIC_URL=http://${ip}:8000", "CASDOOR_REDIRECT_URI=http://${ip}:3010/auth/callback",'POMEGRANATE_DEPLOYMENT_PROFILE=lan',"POMEGRANATE_ACCOUNT_SERVER_URL=http://${ip}:3010")
Set-Content -LiteralPath $target -Value $content -Encoding utf8
Write-Host "已生成 LAN 配置：$target"; Write-Host "网卡：$($context.InterfaceAlias)；地址：$ip；子网：$(Get-SubnetCidr $ip $context.PrefixLength)；网络：Private"
