# Pomegranate 账号系统融合交付说明

生成日期：2026-07-28

本文用于把当前账号系统融合成果交付给团队继续开发、测试和部署。请优先以本文中的“目标账号化仓库”为准，不要把 `D:\ag\firstwork` 或其他来源目录当成已经账号化的交付版本。

## 1. 交付仓库与获取方法

### 1.1 账号系统融合主仓库

- GitHub 仓库：`https://github.com/weixiaochuan/xunfei-manufacturing-tech.git`
- 目标分支：`integration/combined-product-20260724`
- 当前本地 HEAD：`4c251431 feat: harden learning assistant quiz memory`
- 本地 Git 根目录：`D:\ag\pome-combined-integration`
- 本地 App 目录：`D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate`

团队成员获取方式：

```powershell
git clone https://github.com/weixiaochuan/xunfei-manufacturing-tech.git
cd xunfei-manufacturing-tech
git fetch origin
git checkout integration/combined-product-20260724
cd "7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
```

说明：

- 该分支已经被推送到 GitHub，其他成员可以通过上述命令拉取。
- 当前本地目标仓库还有一个未提交文件：`src-tauri/src/services/ppt_master.rs`。这是 PPT 模板词泄漏修复相关工作，不属于已推送的账号系统基线。
- 如需继续 PPT 修复，应单独检查、测试和提交，不要混入账号系统提交。

### 1.2 不作为账号系统交付的目录

- `D:\ag\firstwork\Pomegranate`
- 当前分支：`master`
- 当前 HEAD：`7886023`
- 当前状态：存在大量本地未提交修改和未跟踪文件，且未配置 GitHub 远端。

用途：

- 只作为旧功能、AI 助学/AI 助研/知识图谱/PPT 等来源和对照。
- 不应直接推送给账号系统团队作为可运行账号化版本。
- 后续若要迁入其中功能，必须先做“来源文件 -> 目标模块 -> 数据归属 -> 安全边界”的映射，再按小提交增量合并到账号融合主仓库。

## 2. 当前进度总览

### 2.1 已完成并进入账号融合分支的内容

- Account Server 已新增账号文件、文档、文件夹、学习项目、项目资料关联等能力。
- 已有 migrations 001-009，包括：
  - 账号用户与会话基础；
  - `user_files`；
  - `documents`、`document_folders`；
  - “助学模块上传”目录类型；
  - `learning_projects`；
  - `learning_project_documents`。
- 学习项目 owner 来自服务端 Session，React 不持有 Account Server Token。
- Tauri 已完成账号学习项目安全代理：
  - 项目 CRUD；
  - 项目文档关联、更新、移除、排序；
  - revision 并发控制；
  - 账号切换 envelope 防串号。
- Tauri 已完成 AI 助学安全上传命令：
  - 固定上传到 `folderKind=learning_assistant_upload`；
  - 返回稳定 `documentId`；
  - React 不接触 Token、ownerId、folderId 或本地绝对路径。
- 前端已完成学习项目和项目文档安全 API：
  - 运行时响应校验；
  - documentSession generation 防串号；
  - revision safe integer 校验；
  - `sortOrder` i32 上限校验。
- AI 助学第一版页面已经接入账号项目底座，并继续补齐：
  - ActivityBar 入口；
  - 功能开关；
  - 项目创建、打开、保存、改名、复制、删除；
  - fallback 学习计划；
  - DailyTimeWheelPicker；
  - fallback questions/resources；
  - quiz 细节；
  - QA/对话记忆安全增强；
  - 学习活动、进度、掌握度与重新规划相关增强；
  - 助学上传文件夹显示相关修复。
- TEST Casdoor 配置已支持环境变量中的 organization/application：
  - 不再固定要求正式 `pomegranate` / `app-pomegranate`；
  - 仍要求非空；
  - audience 仍按 Client ID 严格校验；
  - Client Secret 仍只属于 Account Server。

### 2.2 已验证过的关键事实

- 本地 Account Server 测试端口：`127.0.0.1:18080`
- 本地 PostgreSQL 测试端口：`127.0.0.1:55432`
- TEST Casdoor 地址：`http://82.157.119.201:18000`
- TEST Casdoor container 对外映射：`0.0.0.0:18000 -> 8000/tcp`
- 云端 Account Server 访问地址：`http://82.157.119.201:8080`
- 早期云端 Account Server 曾返回学习项目路由 404，说明公网实例未更新到最新学习项目能力；本地验证应优先使用 `127.0.0.1:18080`。

## 3. 当前工作区状态和交付边界

### 3.1 账号融合主仓库当前状态

最近核验结果：

```text
branch: integration/combined-product-20260724
HEAD: 4c251431 feat: harden learning assistant quiz memory
remote: https://github.com/weixiaochuan/xunfei-manufacturing-tech.git
status: M src-tauri/src/services/ppt_master.rs
```

处理建议：

- 已推送给团队的账号系统内容以 `4c251431` 及其之前提交为准。
- `src-tauri/src/services/ppt_master.rs` 是本地未提交修改，不能默认为团队已获取内容。
- 下一次提交前必须先检查：

```powershell
git status --short
git diff --stat
git diff --check
```

### 3.2 firstwork 当前状态

`D:\ag\firstwork\Pomegranate` 当前是功能整合工作区，不是账号系统交付分支。

已知情况：

- 当前本地有大量 Rust、前端、依赖锁文件、资源和业务文件修改。
- 没有远端输出。
- 不建议直接 push，也不建议整体复制到账号融合仓库。

处理建议：

- 只按功能分批迁移：
  - AI 助学文档互通；
  - 课程知识图谱 AI 增强；
  - AI 助研/论文知识库；
  - PPT 修复和生成能力。
- 每批迁移前先做只读差异映射，再在账号融合主仓库按目标架构重接。

## 4. 本地开发和启动方法

### 4.1 Node 与依赖

项目期望 Node 版本：

```text
>=22 <23
```

当前若使用 Node 24，会出现 unsupported engine warning。该 warning 不一定阻塞构建，但正式开发建议切换 Node 22，避免差异。

安装依赖：

```powershell
cd "D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
pnpm.cmd install
```

不要随意更新依赖版本或锁文件。若 `pnpm-lock.yaml` 变化，必须确认是有意变更。

### 4.2 启动本地 PostgreSQL 测试实例

Windows 原生 PostgreSQL 工具目录：

```text
D:\ag\tools\postgresql-17.10\pgsql\bin
```

测试数据目录：

```text
D:\ag\pomegranate-local-test\postgres-data
```

日志目录：

```text
D:\ag\pomegranate-local-test\logs\postgres.log
```

启动命令：

```powershell
& "D:\ag\tools\postgresql-17.10\pgsql\bin\pg_ctl.exe" `
  -D "D:\ag\pomegranate-local-test\postgres-data" `
  -l "D:\ag\pomegranate-local-test\logs\postgres.log" `
  -o "-h 127.0.0.1 -p 55432" `
  start

& "D:\ag\tools\postgresql-17.10\pgsql\bin\pg_isready.exe" -h 127.0.0.1 -p 55432
```

预期：

```text
127.0.0.1:55432 - accepting connections
```

注意：

- 测试数据目录在 Git 仓库外，不要放入项目目录。
- 不要复用来源不明的已有 PostgreSQL 数据目录。
- 不要连接或修改公网数据库。

### 4.3 启动本地 Account Server

本地启动脚本：

```text
D:\ag\pomegranate-local-test\run-account-server.mjs
```

该脚本当前配置：

```text
ACCOUNT_SERVER_HOST=127.0.0.1
ACCOUNT_SERVER_PORT=18080
ACCOUNT_SERVER_PUBLIC_URL=http://127.0.0.1:18080
ACCOUNT_DB_PORT=55432
CASDOOR_PUBLIC_URL=http://82.157.119.201:18000
CASDOOR_REDIRECT_URI=http://127.0.0.1:18080/auth/callback
CASDOOR_ORGANIZATION=pomegranate-test
CASDOOR_APPLICATION=app-pomegranate-test
```

Client ID 和 Client Secret 来源：

```text
D:\ag\pomegranate-local-test\casdoor-client-id.tmp
D:\ag\pomegranate-local-test\casdoor-client-secret.tmp
```

安全要求：

- 不要把这两个文件提交到 Git。
- 不要把值写进源码、文档、终端报告或截图。
- Client Secret 只给 Account Server 使用，不能进入 Tauri、React 或前端构建产物。

启动：

```powershell
node D:\ag\pomegranate-local-test\run-account-server.mjs
```

健康检查：

```powershell
Invoke-WebRequest http://127.0.0.1:18080/health/live
Invoke-WebRequest http://127.0.0.1:18080/health/ready
try {
  Invoke-WebRequest http://127.0.0.1:18080/learning/projects
} catch {
  $_.Exception.Response.StatusCode.value__
}
```

预期：

```text
/health/live -> 200
/health/ready -> 200
未认证 /learning/projects -> 401
```

解释：

- `401` 表示路由存在但需要登录，是正确结果。
- `404` 表示路由不存在，通常说明 Account Server 不是最新代码或没有正确重启。
- `500` 可能是数据库 migration 或配置问题，需要查服务端日志。

### 4.4 启动桌面端

必须在目标 App 目录运行：

```powershell
cd "D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
```

建议使用隔离桌面数据目录，避免旧 SQLite 版本高于当前应用支持版本：

```powershell
$env:KB_DATA_DIR="D:\ag\pomegranate-local-test\desktop-kb-data-42-fresh"
$env:POMEGRANATE_DEPLOYMENT_PROFILE="local"
$env:POMEGRANATE_ACCOUNT_SERVER_URL="http://127.0.0.1:18080"
pnpm.cmd tauri dev
```

常见问题：

- 如果出现 `数据库版本(55)高于应用支持的版本(42)`，说明使用了旧桌面数据目录。换一个新的 `KB_DATA_DIR`，不要删除来源不明的数据。
- 如果浏览器打开 `127.0.0.1:3010/auth/login?...` 拒绝连接，说明桌面端或配置仍指向 3010，而当前本地 Account Server 是 18080。
- 如果 AI 助学新建项目提示“账号服务暂不可用”，优先检查 Account Server 是否还在运行、`/health/ready` 是否 200、桌面端是否连接 `127.0.0.1:18080`。

## 5. 认证与 Casdoor 配置

### 5.1 当前认证链路

当前账号化版本采用：

```text
Casdoor
-> Account Server /auth/callback
-> pomegranate://auth/callback?ticket=...
-> Tauri 使用 ticket 换取安全 Session
```

关键边界：

- React 不持有 Token。
- 桌面端不持有 Client Secret。
- Client Secret 只存在于 Account Server 运行环境。
- Account Server 校验 Token issuer、JWKS、签名算法、organization claim 和 audience。
- audience 必须与 `CASDOOR_CLIENT_ID` 对齐。

### 5.2 TEST Casdoor 后台配置细节

TEST Casdoor 地址：

```text
http://82.157.119.201:18000
```

TEST Organization：

```text
pomegranate-test
```

TEST Application：

```text
app-pomegranate-test
```

本地 Account Server 对应 Redirect URI：

```text
http://127.0.0.1:18080/auth/callback
```

Casdoor 后台需要配置三类对象：

1. Organization

```text
Name: pomegranate-test
Display name: Pomegranate Test
```

说明：

- Account Server 会校验用户所属 organization。
- 如果用户不属于 `pomegranate-test`，回调阶段可能返回 `organization_forbidden`。
- 不要使用正式 organization：`pomegranate`。

2. Application

```text
Name: app-pomegranate-test
Display name: Pomegranate Test
Organization: pomegranate-test
```

OIDC/OAuth 配置：

```text
Grant types: Authorization Code, Refresh Token
Token format: JWT-Custom
Token signing method: RS256
Token fields: Owner, Name, DisplayName, Email
Redirect URI: http://127.0.0.1:18080/auth/callback
```

说明：

- Client ID 从该 Application 的 OIDC/OAuth 页面复制。
- Client Secret 也从该页面复制，但只能写入 Account Server 本地忽略文件或服务器环境变量。
- Redirect URI 必须与 Account Server 实际端口完全一致。
- 当前本地联调端口是 `18080`，所以必须是 `http://127.0.0.1:18080/auth/callback`。
- 如果改用 `3010` 启动 Account Server，Casdoor Redirect URI 和 Account Server `CASDOOR_REDIRECT_URI` 必须同步改成 `http://127.0.0.1:3010/auth/callback`。
- 不要填写 `pomegranate://auth/callback`。这个 deep link 是 Account Server 换取 ticket 后返回给桌面端的，不是 Casdoor 的直接回调地址。

3. Users

测试用户要求：

```text
Organization: pomegranate-test
Application: app-pomegranate-test
User type: normal-user
```

说明：

- 用户必须属于 `pomegranate-test`。
- 如果登录页提示用户不存在，通常是登录到了错误 organization，例如 `built-in` 或正式组织。
- 如果登录成功但 Account Server callback 返回 `organization_forbidden`，通常是用户 token 中的 owner/organization 与 Account Server 配置不一致。

注意：

- Redirect URI 必须精确匹配实际 Account Server 端口。
- 当前本地是 18080，不是 3010。
- 不要写尾部斜杠。
- 不要填 `pomegranate://auth/callback`，深链由 Account Server 回调后再发给桌面端。

### 5.3 本地 Account Server 需要的 Casdoor 配置

当前本地启动脚本从以下位置读取或设置配置：

```text
脚本: D:\ag\pomegranate-local-test\run-account-server.mjs
Client ID: D:\ag\pomegranate-local-test\casdoor-client-id.tmp
Client Secret: D:\ag\pomegranate-local-test\casdoor-client-secret.tmp
```

本地 Account Server 需要的关键值：

```text
CASDOOR_PUBLIC_URL=http://82.157.119.201:18000
CASDOOR_CLIENT_ID=<TEST Application 的 Client ID>
CASDOOR_CLIENT_SECRET=<TEST Application 的 Client Secret>
CASDOOR_ORGANIZATION=pomegranate-test
CASDOOR_APPLICATION=app-pomegranate-test
CASDOOR_REDIRECT_URI=http://127.0.0.1:18080/auth/callback
```

安全要求：

- `CASDOOR_CLIENT_SECRET` 不能进入 React、Tauri、Git diff、截图或报告。
- `CASDOOR_CLIENT_ID` 不是密码，但也建议只写进本地忽略文件，不要硬编码到源码。
- Account Server 才需要 Client Secret；桌面端和前端不需要。
- 如果 `CASDOOR_CLIENT_ID` 配错，Casdoor 授权页面会提示 `invalid_client_id`。
- 如果手动刷新或复制旧 callback 链接，Account Server 可能返回 `invalid_state`。应重新从桌面端点击登录发起新流程。

### 5.4 常见认证错误速查

| 现象 | 常见原因 | 处理方式 |
|---|---|---|
| `/auth/login?client=desktop` 返回 `oidc_unavailable` | Account Server 无法读取 Casdoor discovery，或 `CASDOOR_PUBLIC_URL` 不可访问 | 检查 `http://82.157.119.201:18000/.well-known/openid-configuration`，确认 issuer 是 `http://82.157.119.201:18000` |
| Casdoor 页面显示 `invalid_client_id` | 本地 `casdoor-client-id.tmp` 不是 TEST Application 的 Client ID | 从 `app-pomegranate-test` 的 OIDC/OAuth 页重新复制 Client ID |
| Account Server callback 返回 `invalid_state` | 登录 state 过期、Account Server 重启、手动打开旧 callback URL、或不是从桌面端重新发起 | 关闭旧浏览器页，从桌面端重新点击登录 |
| Account Server callback 返回 `organization_forbidden` | 用户不属于 `pomegranate-test`，或 Account Server `CASDOOR_ORGANIZATION` 配错 | 检查用户 Organization 和 Account Server 配置 |
| 登录后 AI 助学提示账号服务不可用 | 桌面端没有连到本地 Account Server，或 Account Server 已停止 | 检查 `POMEGRANATE_ACCOUNT_SERVER_URL` 和 `http://127.0.0.1:18080/health/ready` |
| `/learning/projects` 未认证返回 404 | Account Server 版本旧，学习项目路由不存在 | 启动目标仓库最新 Account Server，或部署云端新版本 |
| `/learning/projects` 未认证返回 401 | 正常，说明路由存在且认证保护生效 | 登录后继续测试 |

### 5.5 TEST Casdoor 服务器信息

已核到的 TEST Casdoor 部署信息：

```text
SSH 主机: 82.157.119.201
TEST compose 工作目录: /srv/pomegranate-test
TEST compose 文件: /srv/pomegranate-test/compose.casdoor-test.yml
TEST env 文件: /srv/pomegranate-test/.env.casdoor-test
TEST Casdoor app.conf: /srv/pomegranate-test/infra/casdoor-test/casdoor/app.conf
TEST Casdoor 容器: pomegranate-casdoor-test-casdoor-test-1
TEST PostgreSQL 容器: pomegranate-casdoor-test-postgres-test-1
TEST Casdoor 端口: 0.0.0.0:18000 -> 8000/tcp
```

只读诊断命令：

```bash
docker ps --format "table {{.Names}}\t{{.Image}}\t{{.Ports}}\t{{.Status}}"
docker inspect --format '{{ index .Config.Labels "com.docker.compose.project.working_dir" }}' pomegranate-casdoor-test-casdoor-test-1
docker inspect --format '{{ index .Config.Labels "com.docker.compose.project.config_files" }}' pomegranate-casdoor-test-casdoor-test-1
curl -s http://82.157.119.201:18000/.well-known/openid-configuration
```

如需重启 TEST Casdoor，必须确认权限和影响后再执行：

```bash
cd /srv/pomegranate-test
sudo docker compose --env-file /srv/pomegranate-test/.env.casdoor-test \
  -f /srv/pomegranate-test/compose.casdoor-test.yml \
  up -d --force-recreate casdoor-test
```

不要输出 `.env.casdoor-test` 中的 Secret 或密码。

## 6. D:\ag\tools 下两个工具是否可以给其他人使用

`D:\ag\tools` 当前包含两套工具：

```text
D:\ag\tools\postgresql-17.10
D:\ag\tools\casdoor
```

### 6.1 PostgreSQL 工具

可以给其他成员使用。

当前确认存在：

```text
D:\ag\tools\postgresql-17.10\pgsql\bin\psql.exe
D:\ag\tools\postgresql-17.10\pgsql\bin\pg_ctl.exe
D:\ag\tools\postgresql-17.10\pgsql\bin\initdb.exe
D:\ag\tools\postgresql-17.10\pgsql\bin\pg_isready.exe
```

使用方式：

- 可以把整个 `D:\ag\tools\postgresql-17.10` 目录复制给团队成员。
- 每个人应使用自己的数据目录，例如 `D:\ag\pomegranate-local-test\postgres-data`。
- 不要共享同一个 PostgreSQL 数据目录。
- 不要把数据目录、日志、`.tmp` 密码文件放入 Git。

每台机器需要单独初始化或复制已确认安全的空测试数据目录。若目录用途不明，不要复用。

### 6.2 Casdoor Windows 程序包

可以作为“可选本地 Casdoor 调试包”，但当前账号联调默认不使用它。

当前确认存在：

```text
D:\ag\tools\casdoor\casdoor.exe
D:\ag\tools\casdoor\conf\app.conf
D:\ag\tools\casdoor\web\build\
```

当前本地配置状态：

```text
httpport=8000
runmode=dev
origin 为空
默认数据库配置不是当前推荐的账号联调配置
```

结论：

- 其他成员可以复制这套 Casdoor 程序包，但不能直接当成已经配置好的 TEST Casdoor 使用。
- 当前推荐仍然是使用远端 TEST Casdoor：`http://82.157.119.201:18000`。
- 如果某位成员要完全离线运行本地 Casdoor，需要自己配置：
  - Casdoor 数据库；
  - `origin`；
  - organization；
  - application；
  - users；
  - redirect URI；
  - Account Server 的 `CASDOOR_PUBLIC_URL` 和 `CASDOOR_REDIRECT_URI`。
- 本地 Casdoor 的 issuer 会变成本地地址，Account Server 必须同步使用同一个 `CASDOOR_PUBLIC_URL`，否则 token issuer/JWKS 校验会失败。

不建议在当前阶段让每个人各自跑本地 Casdoor，原因是：

- 每个人的 Client ID/Secret 不同；
- issuer 不同；
- 用户和 application 需要重复配置；
- 更容易出现 `invalid_client_id`、`organization_forbidden` 和 redirect mismatch。

推荐团队共用：

```text
Casdoor TEST: http://82.157.119.201:18000
Organization: pomegranate-test
Application: app-pomegranate-test
Local Account Server: http://127.0.0.1:18080
Redirect URI: http://127.0.0.1:18080/auth/callback
```

## 7. 公网 Account Server 与云端部署流程

### 7.1 当前云端端口与实例

已知公网访问地址：

```text
公网 Account Server: http://82.157.119.201:8080
TEST Casdoor: http://82.157.119.201:18000
```

云端容器核验中曾看到：

```text
pomegranate-cloud-account-server-1  pomegranate-account-server:public-ip-test-20260724  3010/tcp
pomegranate-cloud-caddy-1           caddy:2.10.0-alpine                                 0.0.0.0:8000->8000/tcp
pomegranate-cloud-casdoor-1         casbin/casdoor:3.119.0
pomegranate-cloud-postgres-1        postgres:17.6-alpine
```

说明：

- Account Server 容器内部端口是 3010。
- 外部用户之前通过 `http://82.157.119.201:8080` 访问 Account Server。
- 早期公网 Account Server 曾对 `/learning/projects` 返回 404，说明公网运行实例未包含最新学习项目路由，或没有正确部署/重启。

### 7.2 云端更新必须遵守的顺序

在没有明确批准前，不要直接部署、重启或执行公网 migration。

安全更新顺序：

1. 备份 PostgreSQL。
2. 验证备份可读取或可恢复。
3. 记录当前 Account Server 镜像、提交、启动配置和环境变量来源。
4. 构建或准备新版 Account Server 镜像。
5. 在隔离环境验证新版可以启动。
6. 确认 migrations 001-009 哪些已经执行。
7. 只执行仓库已有、尚未执行的 migration。
8. 重启或替换 Account Server。
9. 验证：
   - `GET /health/live`
   - `GET /health/ready`
   - 未认证 `GET /documents` 返回认证错误而不是路由 404；
   - 未认证 `GET /learning/projects` 返回认证错误而不是路由 404；
   - 登录、文档、文件夹、学习项目和资料关联功能正常。
10. 如失败，使用部署前镜像和数据库备份回滚。

错误区分：

- 路由不存在：通常是 404，且未进入认证逻辑。
- 路由存在但未登录：通常是 401。
- 数据表缺失：通常是 500 或数据库错误。
- 业务资源不存在或跨账号：通常是业务 404。

## 8. 功能归属：云端、本地和 Tauri

### 8.1 必须放在云端 Account Server 的能力

这些数据或能力属于账号、需要跨设备、需要权限隔离，应放到 Account Server 和 PostgreSQL：

- 用户、会话、账号身份映射；
- `user_files` 文件元数据；
- `documents` 文档元数据；
- `document_folders` 文件夹；
- “助学模块上传”目录；
- `learning_projects` 学习项目；
- `learning_project_documents` 项目与文档关联；
- 项目 revision 并发控制；
- 后续若要求跨设备同步的：
  - AI 助学对话记忆；
  - 测验记录、答案、评分；
  - 知识点掌握度；
  - 学习活动时间线；
  - 自动重新规划历史；
  - 账号化 AI 助研项目、论文收藏、检索历史；
  - 课程知识图谱 AI 建议和审核结果。

原则：

- owner 只能来自服务端 Session。
- React 不能传 ownerId。
- 跨账号资源统一按不存在处理。
- 不保存本地绝对路径。

### 8.2 应放在 Tauri 本地安全层的能力

这些能力涉及桌面能力、Token 隔离、本地文件选择或本地资源，应由 Tauri 承担：

- 安全保存 Account Server Session；
- 打开系统浏览器登录；
- 接收 deep link ticket；
- 文件选择器；
- 上传文件到 Account Server；
- 固定 AI 助学上传 `folderKind=learning_assistant_upload`；
- Account Server REST API 的类型化安全代理；
- 本地只读知识点 Excel 资源读取；
- 模型配置读取和安全调用代理；
- 本地 fallback 计划/测试/资源生成；
- 桌面 SQLite 本地功能数据。

### 8.3 React 前端只负责的能力

React 只做 UI 和安全 API 封装：

- 页面展示；
- 表单状态；
- 调用 Tauri command；
- 运行时响应校验；
- documentSession generation 防串号；
- Toast 和用户提示；
- 不保存 Token；
- 不直接访问 PostgreSQL；
- 不直接调用需要密钥的外部模型 API。

### 8.4 暂时保持本地或待设计的能力

以下能力目前不应直接上传云端，除非先设计账号归属和数据模型：

- `D:\ag\firstwork` 中尚未整理的功能源码；
- AI 助研论文 PDF 本体和本地分析缓存；
- 课程知识图谱本地运行数据；
- PPT 生成中间项目、SVG、PPTX 输出；
- 桌面本地 SQLite 运行库；
- 构建产物 `dist`、`target`；
- 日志、缓存、临时文件；
- `.env`、Secret、Token、密码。

## 9. 后续待迁移和待开发功能分配

### 9.1 AI 助学

已接入账号底座：

- 项目；
- 资料上传；
- 项目资料关联；
- fallback 计划；
- quiz/QA/进度部分增强。

后续重点：

- 完整 GUI 全链路验收；
- 判断对话、测验、掌握度、时间线是否需要云端跨设备持久化；
- 如果需要云端持久化，应新增 Account Server migration 和 REST API，不要继续只存本地；
- 双账号隔离测试恢复后补测；
- `enabled_views` 旧用户兼容仍是已知风险。

### 9.2 AI 助研

当前主要在 `D:\ag\firstwork\Pomegranate` 中进行过功能整合，不能视为账号融合主仓库已完成。

后续分配建议：

- 本地侧：
  - 论文检索 UI；
  - PDF 分析；
  - 论文知识图谱展示；
  - 模型不可用 fallback；
  - 临时结果展示。
- 云端侧：
  - 如果需要账号化论文知识库、收藏、检索历史、研究项目，应新增 Account Server 数据模型；
  - 论文知识库笔记如需跨设备，应归入账号文档/笔记体系；
  - 不要把用户 PDF 原文件或缓存直接塞进源码仓库。

### 9.3 课程知识图谱 AI 增强

后续分配建议：

- 本地侧：
  - 图谱可视化；
  - AI 建议面板；
  - pending/accepted/rejected 审核 UI；
  - 本地 mock 测试。
- 云端侧：
  - 若图谱关系属于账号或课程共享数据，应设计服务端表；
  - AI 关系建议必须先入建议表，人工接受后再进入正式关系；
  - 需要 migration、索引、唯一约束和权限边界。

### 9.4 PPT 生成

当前账号融合主仓库有本地未提交文件：

```text
src-tauri/src/services/ppt_master.rs
```

后续建议：

- 先完成 PPT 模板词泄漏根因修复；
- 不要关闭泄漏检测；
- 不要把 `Pomegranate` 从全局禁用词中简单移除；
- 修复后单独提交；
- PPT 生成输出目录、SVG 中间产物、PPTX 文件均不应提交。

## 10. 建议验证命令

### 10.1 Account Server

```powershell
cd "D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
pnpm.cmd --dir services\account-server typecheck
pnpm.cmd --dir services\account-server build
pnpm.cmd --dir services\account-server test
pnpm.cmd run test:account-documents
```

### 10.2 前端

```powershell
cd "D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
pnpm.cmd exec tsc --noEmit
pnpm.cmd build
```

### 10.3 Tauri/Rust

```powershell
cd "D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
cargo check --manifest-path src-tauri\Cargo.toml --lib
cargo test --manifest-path src-tauri\Cargo.toml
```

说明：

- `cargo fmt --check` 曾存在历史遗留失败，不能为了通过格式检查而格式化范围外 Rust 文件。
- 如果 GUI 启动报数据库版本过高，优先换隔离 `KB_DATA_DIR`。

### 10.4 Git 和安全扫描

```powershell
git status --short --untracked-files=all
git diff --stat
git diff --check
git diff --name-only
```

提交前必须确认：

- 没有 `.env`；
- 没有 Secret、Token、密码；
- 没有数据库、WAL、日志、缓存、上传文件；
- 没有 `dist`、`target` 等构建产物；
- 没有把 `D:\ag\firstwork` 的运行数据迁入目标仓库；
- 没有使用 `git add .`。

## 11. 推荐提交拆分

后续团队协作时建议按以下顺序拆分：

1. `fix: harden ppt template leak detection`
   - 仅 PPT 模板/安全检查相关文件。
2. `feat: persist learning assistant conversations`
   - 如确认对话记忆需要云端，包含 Account Server migration、service、route、Tauri proxy、前端 API。
3. `feat: persist learning assistant quiz progress`
   - 测验、掌握度、进度和重新规划历史的云端持久化。
4. `feat: integrate account research library`
   - AI 助研论文知识库账号化。
5. `feat: add account course graph AI suggestions`
   - 课程知识图谱 AI 建议和审核流程。
6. `chore: update cloud account server deployment`
   - 仅部署配置、文档和 migration 验证，不混入业务 UI。

每个提交都应：

- 先确认工作区；
- 只暂存明确文件；
- 不使用 `git add .`；
- 测试通过后再提交；
- 不自动 push，除非负责人确认。

## 12. 给下一位开发者的最短启动路线

1. 拉取账号融合分支：

```powershell
git clone https://github.com/weixiaochuan/xunfei-manufacturing-tech.git
cd xunfei-manufacturing-tech
git checkout integration/combined-product-20260724
cd "7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
```

2. 准备本地测试 Secret 文件：

```text
D:\ag\pomegranate-local-test\casdoor-client-id.tmp
D:\ag\pomegranate-local-test\casdoor-client-secret.tmp
```

3. 启动 PostgreSQL：

```powershell
& "D:\ag\tools\postgresql-17.10\pgsql\bin\pg_ctl.exe" `
  -D "D:\ag\pomegranate-local-test\postgres-data" `
  -l "D:\ag\pomegranate-local-test\logs\postgres.log" `
  -o "-h 127.0.0.1 -p 55432" `
  start
```

4. 启动 Account Server：

```powershell
node D:\ag\pomegranate-local-test\run-account-server.mjs
```

5. 启动桌面端：

```powershell
cd "D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate"
$env:KB_DATA_DIR="D:\ag\pomegranate-local-test\desktop-kb-data-42-fresh"
$env:POMEGRANATE_DEPLOYMENT_PROFILE="local"
$env:POMEGRANATE_ACCOUNT_SERVER_URL="http://127.0.0.1:18080"
pnpm.cmd tauri dev
```

6. 登录测试：

- 浏览器应跳转到 `http://82.157.119.201:18000`。
- Account Server callback 应是 `http://127.0.0.1:18080/auth/callback`。
- 登录成功后桌面端显示当前账号。
- AI 助学新建项目应能写入本地 PostgreSQL 测试库。

## 13. 当前风险和注意事项

- `src-tauri/src/services/ppt_master.rs` 尚有本地未提交修改，需要单独收口。
- 公网 Account Server 8080 是否已经更新到最新学习项目能力，仍需由服务器负责人按备份和 migration 流程验证。
- TEST Casdoor 用户和应用配置可以用于本地验证，但 Secret 不得写入仓库。
- `firstwork` 仍是功能整合来源，不是干净交付物。
- Node 24 会有 engine warning，建议团队统一 Node 22。
- 旧用户 `enabled_views` 自动显示 AI 助学入口的兼容风险需要单独任务确认。
- AI 助学完整闭环中哪些数据必须云端持久化，仍需产品确认。
