# Pomegranate 本地账号基础设施

本目录提供本地账号闭环所需的 PostgreSQL 和 Casdoor。Account Server 与桌面登录代码位于仓库其他目录；当前仍不包含课堂、文件共享、云服务器、域名、HTTPS、反向代理或公网访问。

## 服务与端口

| 服务 | 本机地址 | 用途 |
| --- | --- | --- |
| PostgreSQL | `127.0.0.1:5432` | Casdoor 与 Account Server 的服务端数据库 |
| Casdoor | `http://localhost:8000` | 本地身份认证与管理后台 |
| Account Server | `http://127.0.0.1:3010` | OIDC、平台用户映射和桌面 ticket 交换 |

Compose 将两个已发布端口绑定到 `127.0.0.1`，仅供本机开发使用。Pomegranate 客户端不应直接连接 PostgreSQL；后续客户端只与 Account Server 或认证流程通信。

## 镜像版本选择

Casdoor 固定为 `casbin/casdoor:3.119.0`。该版本是 2026-07-22 配置本环境时 Casdoor GitHub 的最新正式发布版（GitHub release 为 `v3.119.0`），Docker Hub 同时提供 `3.119.0` 标签。固定版本可避免 `latest` 后续变化造成不可复现的本地环境。升级时应先查阅新版本发布说明，在单独变更中更新并重新验证，而不是改回 `latest`。

PostgreSQL 固定为 `postgres:17.6-alpine`，避免开发环境随浮动标签发生意外升级。

参考：

- https://github.com/casdoor/casdoor/releases/tag/v3.119.0
- https://hub.docker.com/r/casbin/casdoor/tags
- https://casdoor.org/docs/basic/configuration

## 准备本地环境变量

在项目根目录执行：

```powershell
Copy-Item .env.account.example .env.account
```

然后编辑 `.env.account`，把以下两个占位密码改成同一个仅用于本地开发的强密码：

```text
POSTGRES_PASSWORD
CASDOOR_DB_PASSWORD
```

`POSTGRES_USER` 与 `CASDOOR_DB_USER` 也必须保持一致。`.env.account` 已被 Git 忽略，不得提交；`.env.account.example` 只保留明显的开发占位值。

数据库名称是本项目约定，保持为：

- `casdoor`：保存 Casdoor 的组织、用户、应用、认证配置等数据。
- `pomegranate_account`：由 Account Server 保存稳定平台用户映射；当前不保存 Casdoor token 或长期会话。

## Compose 静态检查

静态检查只解析和展开 Compose，不会启动或下载容器：

```powershell
docker compose --env-file .env.account.example -f compose.account.yml config
```

使用真实本地变量检查：

```powershell
docker compose --env-file .env.account -f compose.account.yml config
```

注意：`config` 会把环境变量展开到终端输出中。使用真实 `.env.account` 时，不要把完整输出粘贴到公开日志或提交到仓库。

## 启动与停止

启动：

```powershell
docker compose --env-file .env.account -f compose.account.yml up -d
```

查看状态和日志：

```powershell
docker compose --env-file .env.account -f compose.account.yml ps
docker compose --env-file .env.account -f compose.account.yml logs postgres
docker compose --env-file .env.account -f compose.account.yml logs casdoor
```

停止但保留数据库：

```powershell
docker compose --env-file .env.account -f compose.account.yml down
```

## Casdoor 首次启动

本轮没有预置 Casdoor 组织、Pomegranate 应用、OAuth/OIDC 回调地址或管理员新密码。首次实际启动后，需要打开 `http://localhost:8000` 完成后台检查和后续应用配置。Casdoor 官方文档说明全新实例可能包含内置管理员初始凭据；若当前固定版本提供该账号，应在首次登录后立即修改密码。仓库没有伪造或保存管理员密码。

当前 Casdoor 已配置 Pomegranate 专用应用，OIDC 回调固定为 Account Server 的本地 callback；真实 Client Secret 只保存在被 Git 忽略的服务级 `.env` 中。

## 完整桌面登录开发流程

1. 启动 PostgreSQL 与 Casdoor：

   ```powershell
   docker compose --env-file .env.account -f compose.account.yml up -d
   ```

2. 启动 Account Server：

   ```powershell
   pnpm --filter @pomegranate/account-server dev
   ```

3. 另开终端启动 Pomegranate：

   ```powershell
   pnpm tauri:dev
   ```

4. 在桌面 Header 点击“登录”，系统默认浏览器将打开 Casdoor。成功后浏览器调用 `pomegranate://auth/callback`，桌面 Rust 后端消费一次性 ticket，Header 显示展示名称与 `POME-` 账号编号。

Tauri 配置只注册 `pomegranate` scheme。Windows NSIS 安装包会根据静态 deep-link 配置注册协议；Windows debug 构建在启动时通过官方插件的 `register_all()` 自动注册当前开发可执行文件，通常不需要管理员权限或额外命令。若开发可执行文件位置发生变化，重新运行 `pnpm tauri:dev` 即可刷新注册。仓库现有单实例机制会把协议启动的辅助进程收到的回调瞬时转交给已运行的默认实例，读取后立即删除，不写入 SQLite、设置存储或日志。

当前账号状态只保存在 React/Zustand 内存中。关闭软件后会恢复未登录状态；当前没有 refresh token、退出、长期会话或重启登录恢复。

## 数据初始化与清理

`001-create-databases.sql` 由 PostgreSQL 官方镜像在命名 volume 首次初始化时执行，创建 `casdoor` 和 `pomegranate_account`。已有数据 volume 再次启动时不会重新执行初始化脚本。

以下命令会连同命名 volume 一起删除，从而永久清空两个数据库中的全部本地账号和认证配置：

```powershell
docker compose --env-file .env.account -f compose.account.yml down -v
```

仅在明确需要完全重建本地账号环境时才使用。普通停止只执行不带 `-v` 的 `down`。

## 常用排错

```powershell
docker info
docker compose version
docker compose --env-file .env.account -f compose.account.yml config
docker compose --env-file .env.account -f compose.account.yml ps
docker compose --env-file .env.account -f compose.account.yml logs postgres
docker compose --env-file .env.account -f compose.account.yml logs casdoor
Get-NetTCPConnection -State Listen -LocalPort 5432,8000 -ErrorAction SilentlyContinue
```

常见问题：

- Docker Engine 未运行：先启动 Docker Desktop，再检查 `docker info`。
- 端口被占用：关闭冲突程序，或只在 `.env.account` 中调整本机映射端口。
- Casdoor 无法连接数据库：确认两处数据库用户名和密码保持一致，并检查 PostgreSQL 已通过健康检查。
- 改了初始化 SQL 但数据库没有变化：初始化脚本只在空数据 volume 上执行；先备份数据，再决定是否进行破坏性的 volume 重建。

生产环境中 PostgreSQL 不得像本地开发环境这样直接暴露公网。应放在受控私有网络内，通过防火墙、安全组和最小权限数据库账号限制访问，并启用适合生产环境的加密、备份和密钥管理。
