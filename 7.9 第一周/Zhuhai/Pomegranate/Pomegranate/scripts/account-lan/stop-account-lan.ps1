param([switch]$StopDataServices)
. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
$root=Get-RepositoryRoot;$pidFile=Join-Path $root '.account-lan\account-server.pid'
if(Test-Path $pidFile){$processId=[int](Get-Content $pidFile -Raw);$process=Get-CimInstance Win32_Process -Filter "ProcessId=$processId" -ErrorAction SilentlyContinue;if($process -and $process.CommandLine -match 'account-server.+dist[/\\]src[/\\]index\.js'){Stop-Process -Id $processId};Remove-Item $pidFile}
if($StopDataServices){$compose=Get-ComposeLanArgs;& docker compose @compose stop casdoor postgres}
Write-Host 'LAN Account Server 已停止；volume 和用户数据均已保留。'
