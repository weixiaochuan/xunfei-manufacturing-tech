# Pomegranate ICP 备案前 Public IP TEST 服务端

本方案只用于 ICP 备案完成前的短期异地联调。它复用服务器上现有的 PostgreSQL 数据卷和私密文件目录，只重建 Account Server 镜像，并重新创建 Casdoor、Account Server、Caddy 容器以加载临时公开地址。它不会迁移真实数据，也不会重建 PostgreSQL。

临时公开地址固定为：

- Account Server：`http://82.157.119.201:8080`
- Casdoor：`http://82.157.119.201:8000`
- OIDC 回调：`http://82.157.119.201:8080/auth/callback`

## 安全边界

- 仅使用全新测试账号和全新测试密码，不得复用正式或个人密码。
- 不上传隐私、课程正式材料、用户真实文件或生产数据。
- 不迁移本地 PostgreSQL、Casdoor 数据或 `user-files`。
- 腾讯云安全组仅在联调期间临时开放 TCP 8000、8080。
- PostgreSQL 5432、Account Server 3010 均不得开放公网。
- ICP 和 HTTPS 生效后立即关闭 8000、8080，并恢复正式 cloud 配置。

严禁执行：

```text
docker compose down -v
docker volume rm
rm -rf /srv/pomegranate/data
docker system prune -a
```

## 1. 备份当前私密配置

服务器部署目录固定为：

```bash
cd /srv/pomegranate/deploy
```

先在服务器本机创建仅 root 可读的配置备份，不要下载、发群或提交 Git：

```bash
sudo install -d -m 0700 /srv/pomegranate/config-backups
sudo cp --preserve=mode,ownership,timestamps \
  .env.cloud \
  "/srv/pomegranate/config-backups/env.cloud.$(date -u +%Y%m%dT%H%M%SZ)"
```

不要在终端输出 `.env.cloud` 内容。

当前磁盘上的 `.env.cloud` 可能已经被人工改成临时 IP 地址，因此这次备份只用于防止继续误改，不能自动视为“正式域名配置备份”。恢复正式模式时，应使用更早的可信备份，或只把公开配置恢复为：

```text
DEPLOYMENT_PROFILE=cloud
ACCOUNT_SERVER_PUBLIC_URL=https://api.stargathering.com
CASDOOR_PUBLIC_URL=https://auth.stargathering.com
CASDOOR_REDIRECT_URI=https://api.stargathering.com/auth/callback
AUTH_DOMAIN=auth.stargathering.com
API_DOMAIN=api.stargathering.com
```

数据库密码、Client ID 和 Client Secret 保持服务器现有私密值，不要复制到命令或文档。

## 2. 放置临时覆盖文件

部署目录中必须具有：

```text
compose.cloud.yml
compose.public-ip-test.yml
infra/cloud/caddy/Caddyfile.public-ip-test
scripts/cloud/public-ip-test-common.sh
scripts/cloud/validate-public-ip-test.sh
scripts/cloud/start-public-ip-test.sh
scripts/cloud/stop-public-ip-test.sh
scripts/cloud/check-public-ip-test.sh
```

如 ZIP 解压后脚本没有执行权限：

```bash
chmod +x scripts/cloud/*.sh
```

## 3. 创建临时环境覆盖

私密密码、数据库账号、Casdoor Client ID 和 Client Secret 继续只保存在服务器现有 `.env.cloud`。临时文件只覆盖公开地址和 profile：

```bash
cp .env.public-ip-test.example .env.public-ip-test
chmod 600 .env.public-ip-test
```

不要把 `.env.cloud` 或 `.env.public-ip-test` 发群或提交 Git。

## 4. 调整 Casdoor Application 回调白名单

Compose 覆盖会把 Casdoor 容器的 `origin` 和 `originFrontend` 设置为 `http://82.157.119.201:8000`，因此 OIDC Discovery 的 issuer 应随临时入口变化。

Casdoor Application 的 Redirect URI 属于应用配置，不能靠环境变量安全修改。使用 Casdoor 官方管理页面打开组织 `pomegranate` 下的 `app-pomegranate`：

1. 保留现有正式 HTTPS 回调。
2. 新增 `http://82.157.119.201:8080/auth/callback`。
3. 不修改 Client ID。
4. 不查看、复制或轮换 Client Secret。
5. 联调结束后删除临时 HTTP 回调，仅保留正式 HTTPS 回调。

不要直接修改 Casdoor PostgreSQL 表。

## 5. 静态检查并启用

完整 Compose 合并顺序固定为：

```bash
docker compose \
  --env-file .env.cloud \
  --env-file .env.public-ip-test \
  -f compose.cloud.yml \
  -f compose.public-ip-test.yml \
  config
```

先运行安全校验：

```bash
./scripts/cloud/validate-public-ip-test.sh
```

确认 PostgreSQL 当前 healthy 且现有命名 volume 可识别后，启动临时入口：

```bash
./scripts/cloud/start-public-ip-test.sh
```

该脚本只会：

- 构建 Account Server 镜像；
- 重新创建 Casdoor，以加载临时 origin；
- 重新创建 Account Server，以加载 `public-ip-test` profile；
- 重新创建 Caddy，使宿主机只发布 8000、8080。

它不会重建 PostgreSQL、执行 migration、删除 volume 或删除用户文件。

## 6. 验收

查看容器状态：

```bash
docker compose \
  --env-file .env.cloud \
  --env-file .env.public-ip-test \
  -f compose.cloud.yml \
  -f compose.public-ip-test.yml \
  ps
```

运行完整检查：

```bash
./scripts/cloud/check-public-ip-test.sh
```

公开验收地址：

```text
http://82.157.119.201:8080/health/live
http://82.157.119.201:8080/health/ready
http://82.157.119.201:8000/.well-known/openid-configuration
```

OIDC Discovery 的 `issuer` 必须是 `http://82.157.119.201:8000`。

## 7. 停用临时入口并恢复正式 HTTPS

先关闭 Caddy 的临时公网入口：

```bash
./scripts/cloud/stop-public-ip-test.sh
```

此操作只停止 Caddy，不删除 PostgreSQL、Casdoor、Account Server、命名 volume 或用户文件。

然后：

1. 从 `/srv/pomegranate/config-backups` 恢复确认无误的正式 `.env.cloud`。
2. 在 Casdoor 管理页面删除临时 HTTP Redirect URI。
3. 从腾讯云安全组撤销 TCP 8000、8080。
4. DNS 和 HTTPS 已准备完成后，运行正式脚本：

```bash
./scripts/cloud/start-services.sh
```

正式 `compose.cloud.yml`、正式 Caddyfile 和 Account Server 的 `cloud` HTTPS 校验均未被本方案放宽。
