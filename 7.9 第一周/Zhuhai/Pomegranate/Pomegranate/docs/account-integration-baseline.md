# Pomegranate 账号黄金集成基座

本文是 Pomegranate 账号、文档、文件与云端部署能力的唯一集成交接说明。黄金基线用于后续功能合并，不作为日常开发分支。

## A. 黄金基线信息

| 项目 | 值 |
|------|----|
| 分支 | `account-golden-baseline-20260724` |
| Commit | 以 `account-golden-baseline-20260724^{commit}` 解析结果为准；提交自身不硬编码自引用哈希 |
| Annotated tag | `account-golden-baseline-20260724` |
| 创建日期 | 2026-07-24 |
| 冻结提交说明 | `chore: freeze account integration baseline` |

本基线接受并冻结以下已完成范围：

- React/Tauri 桌面账号入口、系统浏览器登录与 `pomegranate://auth/callback` Deep Link。
- Rust/Tauri 严格回调校验、一次性 ticket 交换、Windows Credential Manager Session Token 保存。
- Session 恢复、撤销与退出；原始 Session Token 不进入 React、SQLite、URL 或日志。
- Casdoor OIDC Authorization Code、RS256 验签、JWT-Custom claims 校验。
- Casdoor `sub` 到 `platform_users` 的稳定映射及并发安全的 POME 编号分配。
- Account Server、PostgreSQL migrations 001–006、Markdown 文档与用户文件 API。
- 文档/文件软删除、账号归属校验、双账号隔离、外部编辑同步、SHA-256 与 409 冲突保护。
- local、LAN、cloud、public-ip-test 四种部署 profile 及其构建、Compose、Caddy 和验证脚本。
- 正式 cloud HTTPS 配置与备案前 Public IP TEST 临时 HTTP 配置的安全隔离。

尚未纳入本基线：

- 正式域名完成 ICP 备案后的最终上线切换和公网实测。
- 本地或临时环境真实业务数据向正式云环境的迁移、备份与恢复演练。
- 课堂、文件共享以外的新业务模块。

## B. 系统边界

### React/Tauri 客户端

- React 只接收安全的登录状态和最小用户资料。
- Tauri/Rust 负责网络调用、Deep Link、凭据存储、账号文档桥接和外部编辑同步。
- Session Token 只存在于 Rust 侧内存和 Windows Credential Manager，不得传给 React。

### Account Server

- 独立 Node.js + TypeScript HTTP 服务。
- 负责 OIDC 回调、平台用户映射、Session、文档、文件和所有权校验。
- 不属于 `src-tauri`，可被多台客户端共同访问。

### Casdoor

- 负责注册、登录、OAuth/OIDC 授权和身份令牌签发。
- 不承担 Pomegranate 文档、文件或业务数据存储。

### PostgreSQL

- `casdoor` 数据库由 Casdoor 使用。
- `pomegranate_account` 数据库由 Account Server 使用。
- 正式环境不得将 PostgreSQL 端口直接暴露到公网。

### 独立用户文件存储

- 二进制文件内容保存在配置的用户文件目录。
- PostgreSQL 保存文件元数据、归属、大小、SHA-256 和状态。
- 写入流程必须继续使用现有原子写入与冲突检测逻辑。

### Caddy

- 正式 cloud profile 终止 HTTPS，并分别代理 Account Server 与 Casdoor。
- public-ip-test 使用独立临时配置，不得改变正式 Caddyfile 的 HTTPS 语义。

### 部署 profiles

| Profile | 用途 | 安全边界 |
|---------|------|----------|
| `local` | 单机本地开发 | 仅允许本机地址 |
| `LAN` | 局域网联调 | 仅允许显式局域网配置 |
| `cloud` | 正式域名部署 | 只允许 HTTPS 域名，不接受 HTTP、IP 或静默回退 |
| `public-ip-test` | ICP 前短期异地联调 | 只允许已验证的指定公网 IP 与端口；不得作为正式默认值 |

公开测试 IP `82.157.119.201` 只属于 `public-ip-test` profile，不得进入正式 cloud 默认配置。

## C. 用户身份主链

```text
Casdoor sub
  → platform_users.casdoor_subject
  → platform_users.id
  → Session、文档、文件和未来课堂数据
```

- `platform_users.id` 是 Pomegranate 业务数据库中的用户关联主键。
- `casdoor_subject` 来自经过 RS256 签名、issuer、audience 和 expiration 验证的 ID Token `sub`。
- `account_number`（POME 编号）只用于向用户展示，不是关系主键。
- 用户名、邮箱和显示名可以变化，不得作为数据库关联主键。
- 本地环境和云环境出现相同 POME 编号，不代表同一个平台用户；身份必须以对应环境中可信的 `sub` 映射为准。

## D. 账号保护区

没有账号负责人明确确认，不得删除、覆盖或重构：

- `services/account-server/`
- Account Server 认证中间件
- OIDC state、discovery、token 验证与 callback
- 平台 Session 生成、哈希、验证、撤销和过期逻辑
- Windows Credential Manager 凭据封装
- 文档与文件所有权校验
- migrations 001–006
- `src-tauri` 账号命令、网络策略、Deep Link 与账号文档桥接
- cloud 与 public-ip-test 配置
- 用户文件存储路径校验、原子写入、SHA-256 与 409 冲突逻辑

## E. 外部 Casdoor 状态

以下状态位于部署环境中的 Casdoor/PostgreSQL，不能仅靠 Git 仓库完整重建，交接和部署时必须单独核验：

- Organization：`pomegranate`
- Application：`app-pomegranate`
- OAuth grant types：Authorization Code、Refresh Token
- Token format：JWT-Custom
- Signing algorithm：RS256
- Token fields：`Owner`、`Name`、`DisplayName`、`Email`
- Public IP TEST 临时回调地址
- 正式域名上线后的回调地址
- 普通用户自主注册开关、默认组织、`normal-user` 类型及注册字段

真实 Casdoor Client Secret 不得写入本文、Git、日志或客户端。它只能通过部署环境的秘密配置提供给 Account Server。

## F. 合并规则

1. 所有新功能必须从本黄金基线创建独立 integration 分支。
2. 黄金基线本身冻结，不直接进行日常开发。
3. 每次只合入一个功能模块，每个模块使用独立提交。
4. 禁止整目录覆盖 `src/`、`src-tauri/`、`services/` 或 `infra/`。
5. 公共文件必须逐段人工合并，不得机械选择整份 Current 或 Incoming。
6. 新业务必须复用现有平台用户、Session 和认证请求层。
7. 不得建立第二套登录流程、第二套 Session 或第二张 users 主表。
8. 用户文件必须通过现有文件接口读写，不得直接写用户文件目录。
9. 新的 Account Server 数据库 migration 只能从 `007` 开始追加。
10. 已执行的 migrations 001–006 不得修改、改名、删除或重排。

## G. 共享冲突文件

以下文件或区域是高冲突共享点，合并时必须查看 base/current/incoming 三方差异并人工处理：

- `package.json`
- `pnpm-lock.yaml`
- `src/App.tsx`
- `src/Router.tsx`
- Header、Sidebar 及 `src/components/layout/` 账号相关组件
- Zustand 全局 Store，特别是 `src/store/account.ts` 与聚合入口
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tauri*.conf.json`
- Rust command 模块导出、command 注册与 `src-tauri/src/lib.rs`
- Rust 数据库初始化与 schema 入口
- `compose*.yml`
- `infra/cloud/caddy/Caddyfile*`
- `.env*.example` 环境模板

这些共享文件不得直接执行 “Accept Current” 或 “Accept Incoming”；必须保留双方有效语义并完成对应回归测试。

## H. 验收门槛

### 每轮代码集成至少执行

- `pnpm exec tsc --noEmit`
- `pnpm build`
- `cargo check --lib`
- Account Server `typecheck`
- Account Server `build`
- Account Server 全部自动测试
- Public IP 客户端 URL 安全专项测试
- Rust `account_network` 测试
- 文档 adapter 测试
- cloud Compose 静态解析
- public-ip-test Compose 静态解析
- Caddy 配置校验
- Shell 脚本语法检查
- `git diff --check`
- 敏感信息扫描

普通且已知的 Rust 未使用代码 warning 可以记录，但不作为阻塞；任何编译错误、测试失败、配置解析失败或秘密扫描命中都必须阻止合并。

### 账号真实回归至少包含

- 普通用户注册
- OIDC 登录
- Deep Link 返回桌面应用
- Session 恢复
- 退出后不再恢复
- Markdown 创建与保存
- 文件上传与下载
- 外部编辑并同步
- SHA 不一致产生 409 冲突
- 同账号跨电脑同步
- 两个账号数据严格隔离

## I. 明确禁止

- `docker compose down -v`
- `docker volume rm`
- 修改或删除 PostgreSQL volume
- 删除用户文件目录
- 重写 migrations 001–006
- 把 Token 写入 localStorage、SQLite、日志或 URL
- 业务模块直接使用 Casdoor Client Secret
- 用 POME 编号合并本地用户与云端用户
- 把 Public IP TEST 的 HTTP 配置当作正式生产配置
- 提交真实 `.env`、Secret、密码、私钥、数据库、用户文件、备份、日志或构建产物

违反上述保护区、合并规则或禁止项的变更，不得进入账号集成主线。
