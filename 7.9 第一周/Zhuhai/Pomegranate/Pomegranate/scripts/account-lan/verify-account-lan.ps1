. (Join-Path $PSScriptRoot 'AccountLan.Common.ps1')
$root=Get-RepositoryRoot;Import-SafeEnvFile (Join-Path $root '.env.account.lan');$ip=$env:LAN_IP
$live=Invoke-WebRequest -UseBasicParsing "http://${ip}:3010/health/live";$ready=Invoke-WebRequest -UseBasicParsing "http://${ip}:3010/health/ready";$discoveryResponse=Invoke-WebRequest -UseBasicParsing "http://${ip}:8000/.well-known/openid-configuration"
if($live.StatusCode-ne 200-or $ready.StatusCode-ne 200-or $discoveryResponse.StatusCode-ne 200){throw 'LAN 健康检查未全部返回 200。'}
$discovery=$discoveryResponse.Content|ConvertFrom-Json
foreach($field in 'issuer','authorization_endpoint','token_endpoint','userinfo_endpoint','jwks_uri'){$uri=[Uri]$discovery.$field;if($uri.Host-ne $ip){throw "Discovery 字段 $field 未使用 LAN IP。"}}
if(-not(Test-NetConnection $ip -Port 3010 -InformationLevel Quiet)){throw 'TCP 3010 不可达。'};if(-not(Test-NetConnection $ip -Port 8000 -InformationLevel Quiet)){throw 'TCP 8000 不可达。'};if(Test-NetConnection $ip -Port 5432 -InformationLevel Quiet){throw '安全检查失败：5432 可通过 LAN IP 访问。'}
if((& docker inspect --format '{{.State.Health.Status}}' pomegranate-account-local-postgres-1)-ne 'healthy'){throw 'PostgreSQL 当前不是 healthy。'}
$storage=Join-Path $root 'services\account-server\.data\user-files';New-Item -ItemType Directory -Force $storage|Out-Null;$probe=Join-Path $storage '.lan-write-test';[IO.File]::WriteAllText($probe,'ok');Remove-Item $probe
$compose=Get-ComposeLanArgs;$tables=& docker compose @compose exec -T postgres sh -c 'psql -U "$POSTGRES_USER" -d pomegranate_account -Atc "select tablename from pg_tables where schemaname=''public'' and tablename in (''user_sessions'',''documents'',''user_files'') order by tablename"'
if($LASTEXITCODE-ne 0-or @($tables).Count-lt 3){throw '必要的 Session、文档或文件表不存在。'}
Write-Host 'LAN 验证通过：3010/8000 正常，Discovery 使用 LAN 地址，5432 未开放，数据服务与文件目录正常。'
