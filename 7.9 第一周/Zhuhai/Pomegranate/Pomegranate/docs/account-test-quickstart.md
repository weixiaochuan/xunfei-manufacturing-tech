# Pomegranate 独立账号 TEST：零基础搭建与登录

本文用于在一台全新的 Windows 电脑上搭建隔离的账号测试环境。它不会连接正式账号数据库，但远程 Casdoor TEST 使用公网 HTTP，只能使用临时测试账号、临时密码和无敏感文件。

## 1. 这套系统做什么

```text
Pomegranate 桌面端
  → 本地 Account Server：http://127.0.0.1:18080
  → 本地 PostgreSQL：127.0.0.1:55432
  → 远程 Casdoor TEST：http://82.157.119.201:18000
  → 浏览器完成登录
  → http://127.0.0.1:18080/auth/callback
  → pomegranate://auth/callback 返回桌面端
```

- **Casdoor**（统一登录服务）：验证测试用户名和密码，签发经过 RS256 签名的身份令牌。
- **Account Server**（Pomegranate 账号后端）：保存 Client Secret，校验令牌，创建平台用户和 Session。
- **PostgreSQL**（关系数据库）：保存平台用户、Session 哈希、文件元数据和文档数据。
- **Tauri**（桌面应用壳）：打开系统浏览器，通过 Deep Link（自定义链接）接收一次性登录票据。
- **Runtime**（本机运行数据目录）：放在 Git 仓库外，保存数据库、凭据、日志和测试用户文件。

## 2. 安装并检查环境

以下命令都在 Windows PowerShell 中执行。打开方式：开始菜单搜索“PowerShell”，打开普通窗口即可。

### Git

Git（版本管理工具）用于下载源码。安装 Git for Windows 后执行：

```powershell
git --version
```

正常应显示 `git version ...`。若提示找不到命令，重新打开 PowerShell；仍失败则重新安装 Git 并勾选加入 PATH。

### Node.js 22 与 pnpm

Node.js（JavaScript 运行环境）必须是 22.x。安装 Node.js 22 LTS 后执行：

```powershell
node --version
corepack enable
corepack prepare pnpm@11.9.0 --activate
pnpm --version
```

正常应分别显示 `v22...` 和 pnpm 版本。若 Node 不是 22，请切换到 Node 22；不要用更老版本继续。

### Rust 与 Cargo

Rust（Tauri 本机后端语言）和 Cargo（Rust 构建工具）通过 rustup 安装。安装后重新打开 PowerShell并执行：

```powershell
rustc --version
cargo --version
rustup default stable-msvc
```

正常应显示版本号，并使用 `x86_64-pc-windows-msvc` 工具链。

### Windows C++ Build Tools

安装 Visual Studio 2022 Build Tools，勾选“使用 C++ 的桌面开发”和 Windows SDK。它提供 MSVC（Windows C++ 编译器）。安装完成后重启 PowerShell。

### WebView2

WebView2（桌面窗口使用的网页运行时）通常随 Windows 10/11 和 Edge 安装。若 `setup.ps1` 报缺失，请安装 Microsoft Edge WebView2 Evergreen Runtime。

### PostgreSQL 17

准备 PostgreSQL 17 Windows 工具目录。目录中必须存在：

```text
bin\initdb.exe
bin\pg_ctl.exe
bin\pg_isready.exe
bin\psql.exe
bin\postgres.exe
```

本文用 `$PostgresRoot` 表示该目录，例如 `D:\Tools\postgresql-17`。检查：

```powershell
& 'D:\Tools\postgresql-17\bin\postgres.exe' --version
```

正常应显示 `postgres (PostgreSQL) 17.x`。路径不同就替换为自己的路径。

## 3. 下载仓库和指定分支

在 PowerShell 中选择一个源码存放目录。下面示例使用 `D:\Work`，可替换成自己的目录：

```powershell
New-Item -ItemType Directory -Force -Path 'D:\Work' | Out-Null
Set-Location 'D:\Work'
git clone --branch account-testline-20260730 --single-branch https://github.com/weixiaochuan/xunfei-manufacturing-tech.git pome-account-test
$SourceRoot = 'D:\Work\pome-account-test'
$ProjectRoot = Join-Path $SourceRoot '7.9 第一周\Zhuhai\Pomegranate\Pomegranate'
git -C $SourceRoot branch --show-current
git -C $SourceRoot status --short
```

正常结果：分支为 `account-testline-20260730`，状态没有输出。失败时检查网络、GitHub 访问权限和分支名。下一步不要进入其他 Pomegranate 副本，只使用这里的 `$ProjectRoot`。

## 4. 选择仓库外 Runtime

数据库、密码、日志和用户文件会持续变化，不能放进源码目录。选择一个与 `$SourceRoot` 不重叠的目录：

```powershell
$RuntimeRoot = 'D:\WorkData\pome-account-test-runtime'
$PostgresRoot = 'D:\Tools\postgresql-17'
```

最终结构会类似：

```text
<自选目录>\pome-account-test-runtime
├── postgres-data
├── logs
├── user-files
├── desktop-data
├── postgres-password.tmp
├── casdoor-client-id.tmp
├── casdoor-client-secret.tmp
└── test-users.tmp
```

不要把 Runtime 放进 Git 仓库，不要把其中任何文件发到群聊或提交到 GitHub。

## 5. 准备 Casdoor TEST 凭据

在浏览器打开：

```text
http://82.157.119.201:18000
```

由 TEST 管理员进入：

- Organization：`pomegranate-test`
- Application：`app-pomegranate-test`

复制该应用的 Client ID 和 Client Secret。不要使用正式组织 `pomegranate` 或正式应用 `app-pomegranate`。

第一次可先运行下一节的 `setup.ps1`。脚本会创建 Runtime 并明确列出缺失文件；它不会伪造凭据。随后用记事本分别创建：

```text
<RuntimeRoot>\casdoor-client-id.tmp
<RuntimeRoot>\casdoor-client-secret.tmp
```

每个文件只放一个原始值：不写 `KEY=`，不加引号，不加说明文字。

测试账号记录写入：

```text
<RuntimeRoot>\test-users.tmp
```

格式为每行一个 `用户名=<TEST 专用密码>`，不要添加引号。例如只记录字段形式而不把真实密码写进文档：

```text
test001=<TEST-only password>
test002=<TEST-only password>
```

`<Tab>` 表示按一次 Tab 键。只使用 TEST 专用密码，不得复用个人密码或正式环境密码。

## 6. 一键准备非秘密环境

在同一个 PowerShell 窗口执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$ProjectRoot\scripts\account-test\setup.ps1" `
  -ProjectRoot $ProjectRoot `
  -RuntimeRoot $RuntimeRoot `
  -PostgresRoot $PostgresRoot
```

脚本会：

1. 检查 Git、Node 22、pnpm、Rust/Cargo、MSVC、WebView2 和 PostgreSQL 17；
2. 创建仓库外 Runtime；
3. 生成随机 PostgreSQL TEST 密码；
4. 初始化只监听 `127.0.0.1:55432` 的新数据库目录；
5. 用锁文件安装依赖；
6. 通过仓库已有 `build:mcp:debug` 流程生成 `kb-mcp` sidecar（随桌面启动的辅助程序）；
7. 检查三个 TEST 凭据文件。

正常最后会显示准备完成。缺少 TEST 凭据时脚本仍会完成非秘密准备，并给出准确路径；放好三个文件后可重复运行，已有数据库和凭据不会被覆盖。

若脚本在环境检查阶段失败，按错误信息安装对应工具后重开 PowerShell。不要从其他项目手工复制 sidecar。

## 7. 一键启动

确认三个 TEST 凭据文件存在且非空，然后执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$ProjectRoot\scripts\account-test\start-all.ps1" `
  -ProjectRoot $ProjectRoot `
  -RuntimeRoot $RuntimeRoot `
  -PostgresRoot $PostgresRoot `
  -StartDesktop
```

脚本按顺序执行：

1. 启动 PostgreSQL TEST `127.0.0.1:55432`；
2. 执行 migrations（数据库结构升级，已执行项会安全跳过）；
3. 启动 Account Server `127.0.0.1:18080`；
4. 检查健康接口；
5. 在编译前注入18080地址并启动 Tauri 桌面端。

正常应看到 PostgreSQL ready、Account Server ready，随后出现 Pomegranate 窗口。若18080拒绝连接，先看 `<RuntimeRoot>\logs\account-server.stderr.log`，不要把浏览器改到3010。

## 8. 第一次登录

1. 在 Pomegranate 窗口右上角点击“登录”。
2. 系统浏览器应先打开 `http://127.0.0.1:18080/auth/login?client=desktop`，再跳转到 `http://82.157.119.201:18000`。
3. 选择或输入 TEST 用户 `test001` 和 TEST 专用密码。
4. 登录后浏览器经过 `http://127.0.0.1:18080/auth/callback`。
5. 浏览器会调用 `pomegranate://auth/callback`，Windows 应询问或直接打开 Pomegranate。
6. 回到桌面后，Header 应显示测试用户名和 `POME-xxxxxx` 平台账号编号。

浏览器地址中的 `code`、`state` 和 ticket 都是临时敏感数据，不要截图或复制给他人。若浏览器回调成功但桌面没有出现，确认安装/DEV 进程注册了 `pomegranate://` Deep Link，并检查桌面日志。

## 9. 检查是否成功

另开一个 PowerShell 窗口，重新设置三个路径变量后执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$ProjectRoot\scripts\account-test\check-runtime.ps1" `
  -ProjectRoot $ProjectRoot `
  -RuntimeRoot $RuntimeRoot `
  -PostgresRoot $PostgresRoot
```

正常 JSON 至少应满足：

- `casdoorDiscovery: 200`
- `postgresListening: true`
- `postgresPubliclyListening: false`
- `accountServerListening: true`
- `healthLive: 200`
- `healthReady: 200`
- `sessionUnauthenticated: 401`
- `filesUnauthenticated: 401`

桌面登录必须经过18080，而不是3010。`start-desktop.ps1` 在 Tauri 编译前设置 `POMEGRANATE_ACCOUNT_SERVER_URL=http://127.0.0.1:18080`；直接执行裸 `pnpm tauri dev` 会回退到历史默认3010，不能用于本 TEST。

## 10. 安全停止

执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$ProjectRoot\scripts\account-test\stop-all.ps1" `
  -ProjectRoot $ProjectRoot `
  -RuntimeRoot $RuntimeRoot `
  -PostgresRoot $PostgresRoot
```

停止顺序是桌面端 → Account Server → PostgreSQL。脚本不会删除数据库、用户文件或凭据；下次可继续使用同一 Runtime。

## 11. 常见错误

| 现象 | 原因与处理 |
|---|---|
| 缺少 `kb-mcp-...exe` | 没运行准备脚本。重新运行 `setup.ps1`；它使用 `pnpm build:mcp:debug` 生成，不要从其他目录复制。 |
| 浏览器打开3010 | 用了裸 `pnpm tauri dev` 或旧二进制。停止桌面，使用 `start-all.ps1 -StartDesktop` 重新编译启动。 |
| 18080 `connection refused` | Account Server 未启动。检查 PostgreSQL、三个凭据文件和 `logs\account-server.stderr.log`。 |
| 55432未启动 | PostgreSQL 工具路径错误、残留进程或数据目录异常。运行 `pg_isready`，再查看 `logs\postgres.log`。 |
| `postmaster.pid already exists` | 先确认是否真的有 PostgreSQL 进程。若有，使用 `stop-postgres.ps1`；若没有，不要自行删文件或数据库，保留现场让维护人员检查 crash recovery。 |
| Casdoor 18000无法访问 | 浏览器打开 Discovery URL；检查网络和服务器 TEST 状态。不要改连正式8000。 |
| `invalid_client_id` | Client ID 文件不是 `app-pomegranate-test` 的值，或文件带了 `KEY=`/引号/空格。重新从 TEST 应用复制。 |
| `invalid_state` | 登录页过期、重复使用回调或 Cookie 丢失。回到桌面重新点击登录，只完成最新一次流程。 |
| `organization_forbidden` | 用户不属于 `pomegranate-test`，或 TEST 应用 Token Fields 缺少 Owner。由 TEST 管理员修正，不能取消组织校验。 |
| `invalid_id_token` / nbf | TEST 服务器时钟偏差导致令牌尚未生效。当前仅对精确 TEST 地址启用受限容差；仍失败时检查服务器时间，不能关闭签名、issuer、audience 或 exp 校验。 |
| Node 版本错误 | 安装/切换到 Node 22，重开 PowerShell后再运行 setup。 |
| 本地 Compose 不识别 `gw_priority` | Docker Compose 需至少2.33.1；服务器 v5.3.1支持。此问题只影响部署 Casdoor TEST，不影响本地18080/55432启动。 |
| `/learning/projects` 返回404 | 该路由不属于当前账号 TEST 基线。健康检查使用 `/health/live`、`/health/ready`、`/auth/session` 和 `/files`。 |

## 12. 明确禁止连接的环境

本教程只允许：Casdoor TEST 18000、Account Server TEST 18080、PostgreSQL TEST 55432。

禁止误连或修改：

- 正式 Casdoor `:8000`；
- 正式 Account Server/Caddy `:8080`；
- 正式 PostgreSQL 或任何公网5432；
- 正式 Organization `pomegranate`；
- 正式 Application `app-pomegranate`；
- 正式 Cloud 的 `.env`、volume、用户文件和数据库。

公网18000当前使用 HTTP。不得使用真实密码、不得上传隐私或正式文件、不得用于真实用户。
