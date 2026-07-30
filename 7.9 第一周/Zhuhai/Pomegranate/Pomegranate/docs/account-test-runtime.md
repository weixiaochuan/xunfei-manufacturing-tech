# Pomegranate 独立账号 TEST Runtime 技术说明

本文记录可提交、可复现的 TEST 技术基线。零基础操作步骤见 [account-test-quickstart.md](./account-test-quickstart.md)。真实密码、Client Secret、Token、数据库、日志、桌面数据和用户文件不得进入 Git。

## 1. 固定链路与端口

```text
Pomegranate desktop
  → http://127.0.0.1:18080（Account Server TEST）
  → 127.0.0.1:55432（PostgreSQL TEST）
  → http://82.157.119.201:18000（Casdoor TEST）
  → http://127.0.0.1:18080/auth/callback
  → pomegranate://auth/callback
```

| 端口 | 服务 | 暴露范围 | 用途 |
|---:|---|---|---|
| 18000 | 远程 Casdoor TEST | 临时公网 HTTP | `pomegranate-test` 测试用户认证 |
| 18080 | 本地 Account Server TEST | 仅 `127.0.0.1` | OIDC 回调、平台用户、Session、文件与文档 API |
| 55432 | 本地 PostgreSQL TEST | 仅 `127.0.0.1` | 独立账号 TEST 数据库 |
| 2010 | Vite DEV | 本机 | Tauri 开发前端 |
| 3010 | 历史 Account Server 默认值 | 本机 | 本 TEST 禁止回退；必须编译前注入18080 |
| 8000 | 正式/旧 Casdoor | 非本 TEST | 禁止误连 |
| 8080 | 正式/旧 Account Server/Caddy | 非本 TEST | 禁止误连 |
| 5432 | PostgreSQL 标准端口 | 不得公网开放 | 本 TEST 不使用该宿主端口 |

远程18000只适合短期联调，不得复用真实密码或上传敏感文件。

## 2. Casdoor TEST

- Base URL / issuer：`http://82.157.119.201:18000`
- Discovery：`http://82.157.119.201:18000/.well-known/openid-configuration`
- JWKS：由 Discovery 的 `jwks_uri` 自动发现
- Organization：`pomegranate-test`
- Application：`app-pomegranate-test`
- Redirect URI：`http://127.0.0.1:18080/auth/callback`
- Grant Types：Authorization Code、Refresh Token
- Token Format：JWT-Custom
- Signing：RS256
- Token Fields：Owner、Name、DisplayName、Email

Casdoor 验证用户名和密码并签发 OIDC Token。Account Server 才是 confidential client：Client Secret 仅由服务器进程从仓库外文件读取。桌面端不接触 Client Secret、授权码或 Casdoor token。

回调先到 Account Server，以便执行 state 一次性校验、后端 code exchange、JWKS/RS256 验签、issuer、audience、expiration、required claims、组织和 subject 一致性校验。通过后才用60秒、一次性 ticket 跳转 `pomegranate://auth/callback`。

TEST Casdoor 当前存在约8小时的签发时钟偏差。`CASDOOR_NBF_CLOCK_TOLERANCE_SECONDS=28860` 只允许在以下三项同时成立时使用：

1. `DEPLOYMENT_PROFILE=local`；
2. `ALLOW_LOCAL_TEST_CASDOOR=true`；
3. Casdoor origin 精确为 `http://82.157.119.201:18000`。

容差有32,400秒上限，expiration 仍按当前时间严格校验。长期方案是校准服务器时钟并删除该临时容差。

## 3. Account Server 配置

`scripts/account-test/common.ps1` 提供非秘密固定值，Secret 只在启动子进程前从 Runtime 读取。

| 环境变量 | TEST 值/来源 | 阶段 |
|---|---|---|
| `DEPLOYMENT_PROFILE` | `local` | 运行时 |
| `ACCOUNT_SERVER_HOST` | `127.0.0.1` | 运行时 |
| `ACCOUNT_SERVER_PORT` | `18080` | 运行时 |
| `ACCOUNT_SERVER_PUBLIC_URL` | `http://127.0.0.1:18080` | 运行时 |
| `ACCOUNT_DB_HOST` | `127.0.0.1` | 运行时 |
| `ACCOUNT_DB_PORT` | `55432` | 运行时 |
| `ACCOUNT_DB_NAME` | `pomegranate_account_test` | 运行时 |
| `ACCOUNT_DB_USER` | `pomegranate_test_admin` | 运行时 |
| `ACCOUNT_DB_PASSWORD` | `Runtime\postgres-password.tmp` | Secret，运行时 |
| `USER_FILES_ROOT` | `<Runtime>\user-files` | 运行时 |
| `CASDOOR_PUBLIC_URL` | `http://82.157.119.201:18000` | 运行时 |
| `CASDOOR_CLIENT_ID` | `Runtime\casdoor-client-id.tmp` | 运行时 |
| `CASDOOR_CLIENT_SECRET` | `Runtime\casdoor-client-secret.tmp` | Secret，运行时 |
| `CASDOOR_REDIRECT_URI` | `http://127.0.0.1:18080/auth/callback` | 运行时 |
| `CASDOOR_ORGANIZATION` | `pomegranate-test` | 运行时 |
| `CASDOOR_APPLICATION` | `app-pomegranate-test` | 运行时 |
| `ALLOW_LOCAL_TEST_CASDOOR` | `true` | 运行时 |
| `CASDOOR_NBF_CLOCK_TOLERANCE_SECONDS` | `28860` | 受限 TEST 运行时 |
| `POMEGRANATE_DEPLOYMENT_PROFILE` | `local` | Tauri 编译时 |
| `POMEGRANATE_ACCOUNT_SERVER_URL` | `http://127.0.0.1:18080` | Tauri 编译时 |

`src-tauri/src/account_network.rs` 使用 Rust `option_env!`，因此最后两项必须在 `pnpm tauri dev` 编译前设置。`start-desktop.ps1` 负责注入。裸运行 `pnpm tauri dev` 会使用历史3010默认值，不属于 TEST 启动流程。

local profile 默认拒绝公网 Casdoor。仅显式开关允许精确18000地址；其他公网 origin、正式 cloud 与 public-ip-test 安全规则均未放宽。

## 4. PostgreSQL 与 migrations

- 版本：PostgreSQL 17.x。
- 监听：`127.0.0.1:55432`；IPv6明确拒绝；不发布公网。
- 数据目录：`<Runtime>\postgres-data`。
- 数据库：`pomegranate_account_test`。
- 用户：`pomegranate_test_admin`。
- 密码：由 `initialize-postgres.ps1` 生成到仓库外 `postgres-password.tmp`，不会打印或覆盖。
- host/local auth：SCRAM-SHA-256。

| migration | 主要内容 |
|---|---|
| 001 | `platform_users` 与基础唯一约束 |
| 002 | 唯一非空 `casdoor_subject`、并发安全 POME 账号 sequence |
| 003 | `user_sessions`、token hash、过期/撤销/设备字段与索引 |
| 004 | `user_files`、owner、storage key、大小、SHA-256、软删除 |
| 005 | `documents` 与统一 Markdown/上传文件目录 |
| 006 | `document_folders`、`document_tags`、关联表和文档元数据 |

migration 在事务和 PostgreSQL advisory lock 中执行，`schema_migrations` 保存 filename/version/checksum。已执行项再次运行会核对 checksum 后安全跳过，不删除或重建表。

## 5. Session 与用户/文件隔离

- 平台身份键来自验签 ID Token 的 `sub`，不是可变化的用户名。
- `platform_users.casdoor_subject` 唯一；重复登录复用同一平台 UUID 和 POME 编号。
- POME 编号由 PostgreSQL sequence 分配，不使用 `MAX+1`。
- 平台 Session token 使用32字节密码学随机数；数据库只保存 SHA-256 hash。
- 原始 Session token 只交给 Tauri，并保存到 Windows Credential Manager；React 只拿最小用户资料。
- 文件 owner 只从服务端验证后的 Session 获得，前端不能传 `owner_user_id`。
- 文件查询、下载、替换与软删除同时匹配 file ID 和 owner；跨账号统一404。
- 二进制写入 `<Runtime>\user-files`；数据库保存 owner、原始文件名、内部 storage key、MIME、大小、SHA-256、时间和软删除状态。

自动测试覆盖 Session 与跨账号隔离，但真实 `test002` 登录、真实文件上传/下载和跨账号404仍需人工验收，不能标记为 REAL-PASS。

## 6. Runtime 与脚本

推荐结构：

```text
<仓库外目录>\pome-account-test-runtime
├── postgres-data
├── logs
├── user-files
├── desktop-data
├── postgres-password.tmp
├── casdoor-client-id.tmp
├── casdoor-client-secret.tmp
├── test-users.tmp
└── runtime-settings.ps1
```

| 脚本 | 作用 |
|---|---|
| `setup.ps1` | 检查工具、安装锁定依赖、创建 Runtime、初始化 PostgreSQL、生成 sidecar、检查 TEST 凭据 |
| `initialize-runtime.ps1` | 创建 Runtime 目录和本机路径设置 |
| `initialize-postgres.ps1` | 生成随机 DB 密码并初始化独立 cluster |
| `start-postgres.ps1` / `stop-postgres.ps1` | 安全启动/停止55432，不删除数据 |
| `migrate.ps1` | 执行 Account Server 现有 migration |
| `start-account-server.ps1` / `stop-account-server.ps1` | build、migrate、启动/停止18080，Secret 在父环境中随后清除 |
| `start-desktop.ps1` / `stop-desktop.ps1` | 编译前注入18080，使用独立 desktop-data，启动/停止进程树 |
| `check-runtime.ps1` | 检查端口、Discovery、health 与未认证401 |
| `start-all.ps1` / `stop-all.ps1` | 按正确顺序组合启动/停止 |

启动顺序：PostgreSQL → migrations/Account Server → 桌面端。停止顺序相反。

## 7. sidecar 生成

Tauri 的 `externalBin` 需要：

```text
src-tauri\binaries\kb-mcp-x86_64-pc-windows-msvc.exe
```

它是忽略的构建产物，不应从其他工作区复制或提交。使用仓库已有流程：

```powershell
pnpm build:mcp:debug
```

正式构建使用 `pnpm build:mcp`。`setup.ps1` 在干净 clone 中发现 sidecar 缺失时自动执行 debug 生成流程。

## 8. 健康检查

| 路由 | 未认证预期 | 说明 |
|---|---:|---|
| `/health/live` | 200 | 进程存活，不依赖数据库 |
| `/health/ready` | 200 | 执行数据库 readiness 查询 |
| `/auth/session` | 401 | 没有 Bearer Session 时必须拒绝 |
| `/files` | 401 | 没有 Session 时必须拒绝 |
| `/documents` | 401 | 没有 Session 时必须拒绝 |

`/learning/projects` 不存在于当前账号 TEST 基线，不能用作必需健康检查。

运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\account-test\check-runtime.ps1 `
  -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
```

正常结果：Discovery 200、55432仅 loopback、live/ready 200、未认证 session/files 401。

## 9. Casdoor TEST 部署隔离

`compose.casdoor-test.yml` 使用两个独立网络：

- `casdoor_test_backend`：`internal: true`，只供 `postgres-test` 与 `casdoor-test` 通信；
- `casdoor_test_edge`：普通 bridge，只连接 `casdoor-test`，用于发布 `0.0.0.0:18000:8000`。

PostgreSQL 只连 backend 且没有 `ports`。Casdoor 同时连接 backend/edge，`gw_priority` 为 backend 0、edge 1。Docker Compose 至少需要2.33.1；已知服务器 Compose v5.3.1支持，本地 v2.32.4不支持该字段。

该 TEST 项目不得共享正式 network、volume、数据库、Client Secret 或 Caddy。

## 10. 验收状态

### 已真实验证

- Casdoor TEST Discovery 200。
- PostgreSQL 55432曾真实 ready，migrations 001–006真实执行并重复安全跳过。
- Account Server 18080 live/ready 200。
- `test001` 完成 Casdoor登录、18080 callback、Deep Link、ticket交换、平台用户与 Session创建。
- 桌面 Header 显示测试用户和 POME编号。

### 自动测试/静态验证通过

- RS256、issuer、audience、required claims、exp 与受限 nbf容差。
- 平台用户重复登录与并发安全。
- Session token只存哈希、撤销和设备独立。
- 文件 owner和跨账号404。
- Rust账号网络、Deep Link和凭据封装。

### 尚未人工完成

- 完全关闭后 Session自动恢复。
- 真实退出并重启确认不恢复。
- `test002` 真实登录。
- 两个账号真实上传、下载、删除及跨账号404。

## 11. 禁止提交与正式环境边界

可以提交源码、migration、测试、安全示例、脚本、Compose和无秘密文档。

禁止提交：

- Client Secret、测试密码、Token、真实 `.env`；
- `*.tmp` 凭据；
- `postgres-data`、`user-files`、`logs`、`desktop-data`；
- `node_modules`、`dist`、`target`；
- sidecar EXE、安装包和缓存；
- 本机专用绝对路径配置。

TEST 只使用18000/18080/55432。禁止连接或修改正式 Casdoor 8000、正式 Account Server/Caddy 8080、正式 PostgreSQL、Organization `pomegranate`、Application `app-pomegranate`、正式 Cloud `.env`、volume和用户数据。
