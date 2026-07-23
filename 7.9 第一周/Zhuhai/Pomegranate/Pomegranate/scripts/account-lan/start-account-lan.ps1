param([string]$InterfaceAlias)
. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
$context=Get-PomegranateLanContext -InterfaceAlias $InterfaceAlias; Assert-PrivateLan $context
$root=Get-RepositoryRoot; Import-SafeEnvFile (Join-Path $root '.env.account.lan')
if($env:LAN_IP -ne $context.IPAddress){throw '.env.account.lan 的 LAN_IP 与当前选定物理网卡不一致，请重新运行 prepare-lan.ps1。'}
docker info *> $null; $compose=Get-ComposeLanArgs; & docker compose @compose up -d
if($LASTEXITCODE -ne 0){throw 'PostgreSQL/Casdoor 启动失败。'}
$deadline=(Get-Date).AddMinutes(2); do{$health=& docker inspect --format '{{.State.Health.Status}}' pomegranate-account-local-postgres-1 2>$null;if($health -eq 'healthy'){break};Start-Sleep -Seconds 2}while((Get-Date)-lt $deadline)
if($health -ne 'healthy'){throw 'PostgreSQL 未在两分钟内进入 healthy。'}
$stateDir=Join-Path $root '.account-lan';New-Item -ItemType Directory -Force -Path $stateDir|Out-Null;$pidFile=Join-Path $stateDir 'account-server.pid'
if(Test-Path $pidFile){$oldPid=[int](Get-Content $pidFile -Raw);if(Get-Process -Id $oldPid -ErrorAction SilentlyContinue){throw 'LAN Account Server 已在运行。'};Remove-Item $pidFile}
if(Get-NetTCPConnection -LocalPort 3010 -State Listen -ErrorAction SilentlyContinue){throw '3010 已被其他进程占用，请先停止旧的本机 Account Server。'}
& pnpm --filter '@pomegranate/account-server' build;if($LASTEXITCODE -ne 0){throw 'Account Server 构建失败。'}
$service=Join-Path $root 'services\account-server';$node=(Get-Command node).Source
$process=Start-Process -FilePath $node -ArgumentList 'dist/src/index.js' -WorkingDirectory $service -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $stateDir 'account-server.log') -RedirectStandardError (Join-Path $stateDir 'account-server-error.log')
Set-Content -LiteralPath $pidFile -Value $process.Id -Encoding ascii;Start-Sleep -Seconds 2
if($process.HasExited){throw 'Account Server 启动失败，请查看 .account-lan 下的安全日志。'}
Write-Host "LAN 服务已启动：$($env:ACCOUNT_SERVER_PUBLIC_URL)；Casdoor：$($env:CASDOOR_PUBLIC_URL)"
