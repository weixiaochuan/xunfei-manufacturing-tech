. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
$rules=@(Get-NetFirewallRule -DisplayGroup 'Pomegranate LAN Dev' -ErrorAction SilentlyContinue)
if($rules.Count -eq 0){Write-Host '未找到 Pomegranate LAN Dev 防火墙规则。';exit 1}
foreach($rule in $rules){$port=$rule|Get-NetFirewallPortFilter;$address=$rule|Get-NetFirewallAddressFilter;[pscustomobject]@{Name=$rule.DisplayName;Enabled=$rule.Enabled;Profile=$rule.Profile;Port=$port.LocalPort;RemoteAddress=($address.RemoteAddress-join ',')}}
if($rules|Where-Object {($_|Get-NetFirewallPortFilter).LocalPort -contains 5432}){throw '检测到项目规则错误开放了 5432。'}
