# Casdoor TEST 独立环境部署

该环境仅用于 AI 助学及其他功能的短期账号联调。它只包含独立的 PostgreSQL 和 Casdoor，不包含 Account Server 或 Caddy，也不读取正式 Cloud 环境文件。当前 Casdoor 通过宿主机 `0.0.0.0:18000` 提供临时公网 HTTP 测试入口；该入口不得用于正式用户、真实密码或敏感数据。

## 隔离边界

| 资源 | Casdoor TEST |
|------|--------------|
| Compose project | `pomegranate-casdoor-test` |
| PostgreSQL service | `postgres-test` |
| Casdoor service | `casdoor-test` |
| PostgreSQL database | `casdoor_test` |
| PostgreSQL application role | `casdoor_test_app` |
| Named volume | `pomegranate_casdoor_test_postgres_data` |
| Backend network | `pomegranate_casdoor_test_backend`（`internal: true`） |
| Edge network | `pomegranate_casdoor_test_edge`（独立非 internal bridge） |
| Casdoor image | `casbin/casdoor:3.119.0` |
| Host binding | `0.0.0.0:18000`（临时公网 HTTP） |
| Organization | `pomegranate-test` |
| Application | `app-pomegranate-test` |

测试栈不得加入 `pomegranate_backend`、`pomegranate_edge`，不得挂载正式 PostgreSQL volume，也不得读取 `.env.cloud`。测试 Casdoor 的数据库密码、管理员密码和后续创建的 Client Secret 必须全部重新生成。

测试环境采用双网络结构。`postgres-test` 只连接 `pomegranate_casdoor_test_backend`；`casdoor-test` 同时连接 backend 与 `pomegranate_casdoor_test_edge`，并通过 `gw_priority: 1` 选择 edge 作为默认网关。原因是 Docker 29.6.1 不会为仅连接 `internal: true` 网络的容器落实宿主机端口发布：Compose 中虽然仍能看到 `ports` 声明，容器运行态的 `NetworkSettings.Ports` 却会是 `null`。独立 edge 网络只负责发布 Casdoor TEST 的 `18000 -> 8000`，不会连接或共享正式 `pomegranate_edge`；PostgreSQL 仍留在 internal backend 且没有宿主机端口。

该配置使用 Compose 的 `gw_priority`，部署机必须使用 Docker Compose 2.33.1 或更高版本；校验脚本会在版本过旧时明确拒绝继续。

公网暴露范围只能是测试 Casdoor 的 TCP `18000`。云防火墙和主机防火墙必须把来源限制为参与联调人员的固定公网 IP；不得向 `0.0.0.0/0` 长期开放。容器端口 `8000`、备用端口 `8001` 和 PostgreSQL `5432` 均不得直接开放。联调结束后应立即关闭安全组规则并恢复回环或 HTTPS 方案。

当前 Casdoor 镜像没有单独的持久化数据挂载；Casdoor 数据保存在独立的 `postgres-test` volume 中。不要猜测镜像内部路径并挂载正式 Casdoor 数据。

## 服务器部署目录

建议把测试环境部署在独立目录 `/srv/pomegranate-test/`，不要解压或复制到正式 Cloud 的部署目录：

```text
/srv/pomegranate-test/
├── compose.casdoor-test.yml
├── .env.casdoor-test
├── .env.casdoor-test.example
├── infra/
│   └── casdoor-test/
│       ├── casdoor/
│       │   └── app.conf
│       └── postgres/
│           └── 001-init-casdoor-test.sh
└── scripts/
    └── casdoor-test/
        ├── common.sh
        ├── validate-casdoor-test.sh
        ├── start-casdoor-test.sh
        └── check-casdoor-test.sh
```

需要上传的仓库文件只有：

```text
compose.casdoor-test.yml
.env.casdoor-test.example
infra/casdoor-test/
scripts/casdoor-test/
```

不要上传 `.env.casdoor-test`、`.env.cloud`、正式 Caddy 配置、正式 Account Server 配置、数据库文件、Docker volume、日志或用户数据。

## 完整 SSH 部署步骤

以下命令中的 `<server-user>` 和 `<server-host>` 必须替换为服务器实际 SSH 用户与地址。命令不会修改正式 Cloud，但执行启动步骤前仍应确认当前目录为 `/srv/pomegranate-test`。

### 1. 登录服务器并创建目录

```bash
ssh <server-user>@<server-host>
sudo install -d -m 0750 -o "$USER" -g "$USER" /srv/pomegranate-test
mkdir -p /srv/pomegranate-test/infra /srv/pomegranate-test/scripts
exit
```

### 2. 从开发电脑上传测试环境文件

在包含这些文件的 Pomegranate 项目根目录执行：

```bash
scp compose.casdoor-test.yml .env.casdoor-test.example \
  <server-user>@<server-host>:/srv/pomegranate-test/
scp -r infra/casdoor-test \
  <server-user>@<server-host>:/srv/pomegranate-test/infra/
scp -r scripts/casdoor-test \
  <server-user>@<server-host>:/srv/pomegranate-test/scripts/
```

不要把整个开发工作区、Git 历史或任何真实环境文件一起上传。

### 3. 重新登录并检查上传结果

```bash
ssh <server-user>@<server-host>
cd /srv/pomegranate-test
pwd
find . -maxdepth 4 -type f -print | sort
```

应只看到上方目录结构中的 Compose、示例环境变量、Casdoor/PostgreSQL 配置和脚本。此时不要复制或输出任何真实密码。

### 4. 创建私密环境文件并设置权限

```bash
cp .env.casdoor-test.example .env.casdoor-test
chmod 600 .env.casdoor-test
chmod 750 scripts/casdoor-test/*.sh
chmod 750 infra/casdoor-test/postgres/*.sh
```

使用服务器本地编辑器打开 `.env.casdoor-test`，分别填写两个不同的强随机测试密码。不要复用正式数据库密码、正式管理员密码或正式 Client Secret。

确认私密文件不会被其他普通用户读取：

```bash
ls -l .env.casdoor-test
```

### 5. 运行静态验证

```bash
cd /srv/pomegranate-test
bash ./scripts/casdoor-test/validate-casdoor-test.sh
```

只有看到 `Casdoor TEST environment and Compose configuration are valid.` 才能继续。验证失败时不要绕过脚本，也不要修改正式 Cloud 配置。

### 6. 启动测试环境

```bash
cd /srv/pomegranate-test
bash ./scripts/casdoor-test/start-casdoor-test.sh
```

该步骤只启动 `postgres-test` 和 `casdoor-test`。不要额外组合 `compose.cloud.yml`，也不要指定正式 Compose project。

### 7. 执行运行验证

```bash
cd /srv/pomegranate-test
bash ./scripts/casdoor-test/check-casdoor-test.sh
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  ps
```

额外确认测试栈没有发布 PostgreSQL、8000 或 8001：

```bash
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  port postgres-test 5432
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  port casdoor-test 8000
```

第一条命令应无映射；第二条必须只显示 `0.0.0.0:18000`。如果出现宿主机 `8000`、`8001`、`5432`、额外端口或 IPv6 公网映射，立即停止验收，不得继续创建账号。

### 8. 限制临时公网来源

在云平台安全组和服务器防火墙中，仅允许参与联调人员的固定公网 IP 访问 TCP `18000`。不要开放 TCP `8000`、`8001` 或 `5432`，也不要为该测试入口修改正式 Caddy。

### 9. 浏览器访问 Casdoor TEST

在获准的测试电脑浏览器访问：

```text
http://<server-public-ip>:18000
```

这是未加密的临时 HTTP 入口，只能使用临时账号、独立临时密码和无敏感数据。不要给测试环境添加正式 Caddy 路由。

## 创建私密环境文件

在包含 `compose.casdoor-test.yml` 的项目目录执行：

```bash
cp .env.casdoor-test.example .env.casdoor-test
chmod 600 .env.casdoor-test
```

分别生成两个不同的随机密码：

```bash
openssl rand -hex 32
openssl rand -hex 32
```

替换 `.env.casdoor-test` 中所有 `CHANGE_ME` 值。真实环境文件不得提交、发送到聊天或与 `.env.cloud` 合并。

Casdoor Application 的 Client Secret 不属于本 Compose。首次启动后，应在 Casdoor 管理页面创建测试 Application，并把 Secret 仅写入使用它的测试后端私密环境文件。

## 静态验证

静态检查不会启动容器：

```bash
bash ./scripts/casdoor-test/validate-casdoor-test.sh
```

它会拒绝占位密码、共享密码、错误数据库名称、非预期公网绑定、额外服务、正式网络、正式数据目录和非预期端口。

也可直接查看渲染结果：

```bash
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  config
```

检查渲染结果时不得把包含密码的完整配置复制到聊天或日志。

## 启动

```bash
bash ./scripts/casdoor-test/start-casdoor-test.sh
```

脚本按顺序执行：

1. 校验私密环境和 Compose 隔离规则。
2. 启动 `postgres-test` 并等待 `healthy`。
3. 启动 `casdoor-test`。
4. 验证 volume、网络、数据库、临时公网端口和 OIDC Discovery。

PostgreSQL 不映射宿主机端口。Casdoor TEST 临时映射为 `0.0.0.0:18000`，必须同时通过云安全组和主机防火墙限制来源 IP。

## 首次 Casdoor 配置

通过获准测试电脑访问：

```text
http://<server-public-ip>:18000
```

在 Casdoor 官方管理页面完成：

1. 按当前版本支持的流程初始化或登录测试管理员。
2. 立即为测试管理员设置独立密码，不复用正式账号密码。
3. 创建 Organization：`pomegranate-test`。
4. 创建 Application：`app-pomegranate-test`。
5. 只填写测试服务回调地址，不得填写正式 Account Server 回调。
6. 在 `pomegranate-test` 中创建测试账号 `test001` 和 `test002`，用户类型保持普通用户。
7. 测试账号密码必须是独立临时密码，不得复用任何正式用户密码。
8. 为 AI 助学等独立后端分别创建独立 Application 和 Client Secret。
9. 不创建、导入或复制任何正式用户，也不授予测试账号管理员权限。

当前正式 Account Server 只接受 `pomegranate` 和 `app-pomegranate`。不要修改正式 Account Server 来接受测试 Organization，也不要把它的 Casdoor URL 改为测试地址。

## 运行验证

```bash
bash ./scripts/casdoor-test/check-casdoor-test.sh
```

预期结果：

- `postgres-test` 为 `healthy`。
- `casdoor-test` 为 `running`。
- PostgreSQL 只使用 `pomegranate_casdoor_test_postgres_data`。
- `postgres-test` 只连接 `pomegranate_casdoor_test_backend`。
- `casdoor-test` 只连接 `pomegranate_casdoor_test_backend` 和 `pomegranate_casdoor_test_edge`。
- PostgreSQL 没有宿主机端口。
- Casdoor TEST 临时绑定 `0.0.0.0:18000`，且云安全组只允许批准的来源 IP。
- `casdoor_test` 数据库和 `casdoor_test_app` 角色真实存在。
- OIDC Discovery issuer 与 `.env.casdoor-test` 中的 `CASDOOR_TEST_PUBLIC_URL` 完全一致；当前公网联调环境为 `http://82.157.119.201:18000`。

Organization、Application、回调白名单和 Client Secret 必须在管理页面中人工核验，因为这些状态保存在测试数据库中，不属于 Git 配置。

## 交付给功能整合同学

部署和人工配置全部验收后，可复制以下模板发送。不要把测试密码或 Client Secret 直接填入普通聊天消息，应通过团队批准的秘密传递渠道单独发送。

```text
Pomegranate Casdoor TEST 已可用于功能联调。

访问方式：
- Casdoor TEST 地址：http://<server-public-ip>:18000
- 仅批准的测试来源 IP 可以访问
- 当前为临时公网 HTTP，不具备传输加密

测试租户：
- Organization：pomegranate-test
- Application：app-pomegranate-test
- 测试账号：test001、test002
- 测试密码：通过批准的秘密传递渠道单独获取
- Client ID / Client Secret：由各测试后端负责人通过私密环境变量配置，不得写入前端或仓库

使用限制：
- 只允许测试账号、测试密码和无敏感数据
- 不得创建或导入正式用户
- 不得连接正式 Organization pomegranate 或正式 Application app-pomegranate
- 不得连接正式 Account Server、正式 PostgreSQL、正式 volume 或正式网络
- 只允许临时开放宿主机 TCP 18000，并限制来源 IP
- 不得开放宿主机 TCP 8000、8001、5432 或其他数据库端口
```

## 停止和重新启动

停止但保留测试数据库：

```bash
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  stop
```

移除测试容器和测试网络但保留 named volume：

```bash
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  down
```

不得运行 `down -v`，除非测试环境负责人明确批准删除 `pomegranate_casdoor_test_postgres_data`。任何情况下都不得删除或挂载正式 Cloud PostgreSQL volume。

## 明确不包含

- 正式 Caddy 或正式 HTTPS 入口
- 正式 Account Server
- 正式 PostgreSQL database 或 volume
- 正式 Casdoor 用户和配置
- 正式 Client Secret
- 真实密码或 `.env.casdoor-test`
- AI 助学业务接口

当前 `0.0.0.0:18000` 只用于临时公网测试。联调结束后必须关闭云安全组规则并停用该入口。如未来需要长期公网 HTTPS，应使用独立服务器或独立入口重新评审；不得直接修改正式 Caddyfile，也不得让测试 HTTP 配置成为正式默认值。
