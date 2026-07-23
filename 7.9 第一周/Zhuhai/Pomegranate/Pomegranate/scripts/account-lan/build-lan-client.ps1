param([string]$LanIp='192.168.31.210')
. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
$parsed=$null;if(-not[Net.IPAddress]::TryParse($LanIp,[ref]$parsed)){throw 'LanIp 不是有效 IP 地址。'}
$env:POMEGRANATE_DEPLOYMENT_PROFILE='lan';$env:POMEGRANATE_ACCOUNT_SERVER_URL="http://${LanIp}:3010"
Push-Location (Get-RepositoryRoot)
try{& pnpm tauri build --config 'src-tauri/tauri.lan.conf.json';if($LASTEXITCODE-ne 0){throw 'LAN TEST 安装包构建失败。'}}finally{Pop-Location}
