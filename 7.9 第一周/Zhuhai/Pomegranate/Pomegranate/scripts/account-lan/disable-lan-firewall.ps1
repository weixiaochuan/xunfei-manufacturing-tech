. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
Assert-Administrator
Get-NetFirewallRule -DisplayGroup 'Pomegranate LAN Dev' -ErrorAction SilentlyContinue|Remove-NetFirewallRule
Write-Host '已删除本项目创建的 Pomegranate LAN Dev 规则；其他防火墙规则未改动。'
