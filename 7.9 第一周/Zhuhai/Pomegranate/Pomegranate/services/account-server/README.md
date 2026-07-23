# Pomegranate Account Server

Account Server 是供多台 Pomegranate 客户端共同访问的平台 HTTP 后端。它是独立的 Node.js + TypeScript 进程，默认只监听本机 `127.0.0.1:3010`。

## 与其他组件的区别

- `src-tauri`：桌面应用在当前电脑上的 Rust 后端，负责本地能力；Account Server 不放在其中。
- Casdoor：身份认证系统；Account Server 通过 OIDC Authorization Code 流程向它确认身份。桌面登录恢复使用 Pomegranate 自己的 session，不保存 Casdoor token。
- PostgreSQL：保存平台账号数据。本服务只连接 `pomegranate_account`，不连接 Casdoor 使用的 `casdoor` 数据库。
- SQLite：桌面端现有本地数据；本服务不读取也不修改它。

## 当前范围

已经实现：

- 集中读取和校验环境变量。
- PostgreSQL 连接池及启动连通性检查。
- 可重复执行、带事务和校验和保护的 SQL migration。
- `schema_migrations` 与 `platform_users` 表。
- 以经过验签的 Casdoor `sub` 创建或复用稳定的平台用户。
- 通过 PostgreSQL sequence 分配 `POME-000001` 格式的平台账号编号。
- 不依赖数据库的 `GET /health/live`。
- 执行 `SELECT 1` 的 `GET /health/ready`。
- Ctrl+C / 终止信号下的优雅关闭。
- 使用 OIDC Discovery 获取 Casdoor 的授权、令牌和 UserInfo 端点。
- `GET /auth/login` 生成一次性 state 并跳转到 Casdoor。
- `GET /auth/callback` 在后端换取令牌并返回最小用户资料。
- `GET /auth/login?client=desktop` 创建固定桌面登录流程。
- `POST /auth/desktop/exchange` 原子消费短时一次性 ticket，并返回最小平台用户资料。
- `user_sessions` 表、7 天有效的平台 session、`GET /auth/session` 和 `POST /auth/logout`。
- 桌面端通过系统安全凭据存储保存平台 session token，React 不接触 token。
- `user_files` 元数据表和只允许当前平台用户访问的个人文件 API。

尚未实现：
- Casdoor access token、refresh token 或 ID token 持久化（设计上也不应放入桌面端）。
- refresh token、session 自动续期、全部设备退出或复杂会话管理。
- 云服务器、域名、HTTPS 和 Account Server 容器化。

## 本地配置

在仓库根目录执行：

```powershell
Copy-Item services/account-server/.env.example services/account-server/.env
```

然后只在新建的 `.env` 中手工填写本地 PostgreSQL 的用户名和密码，使 `ACCOUNT_DB_USER`、`ACCOUNT_DB_PASSWORD` 与当前账号基础设施相匹配。不要复制、粘贴或提交真实密码到代码、README、Issue 或日志中。服务级 `.env` 已被仓库根目录的 `.gitignore` 忽略。

当前使用 PostgreSQL 超级用户仅限本地开发。未来部署前必须创建只拥有 `pomegranate_account` 所需权限的最小权限用户。

### OIDC 本地变量

服务级 `.env` 还需要填写 Casdoor Client ID 和 Client Secret。真实 Client Secret 只能放在这个被 Git 忽略的文件中，不得放入源码、示例配置或日志。

本地固定配置为：

- Casdoor：`http://127.0.0.1:8000`
- 回调：`http://127.0.0.1:3010/auth/callback`
- 组织：`pomegranate`
- 应用：`app-pomegranate`

## 最小 OIDC 登录流程

OIDC 是建立在 OAuth 2.0 之上的身份层。当前使用 Authorization Code 流程：浏览器只把短时授权码带回 Account Server，Client Secret 始终留在后端。后端通过 Discovery 获取 issuer、JWKS、令牌和 UserInfo 端点，使用 Casdoor 公钥验证 RS256 ID Token 的签名、issuer、audience 和有效期；平台身份和资料来自验签后的 ID Token，UserInfo 只用于确认 subject 一致。

`state` 是每次登录随机生成的一次性值，用于阻止登录 CSRF。它保存在短时 Cookie 中，并在 callback 校验后立即消费。Cookie 使用 `HttpOnly` 和 `SameSite=Lax`，JavaScript 无法读取；当前本地 HTTP 开发环境不能启用 `Secure`。生产环境必须改用 HTTPS，并启用 Secure Cookie。

开始登录：

```powershell
Start-Process http://127.0.0.1:3010/auth/login
```

浏览器 callback 不会返回任何令牌。首次登录会创建 `platform_users` 记录；同一验签 `sub` 再次登录会复用相同平台 UUID 和账号编号，并更新可变资料。桌面流程随后用一次性 ticket 换取 Pomegranate 平台 session。

组织校验采用“无法证明就拒绝”的策略，并且只读取验签成功后的 ID Token claims。当前兼容明确的 `owner`、`organization` 和字符串数组 `organizations`；用户名、显示名、UserInfo 或未验签 token 内容不能作为组织依据。真实 Casdoor token 使用 `owner` 证明用户属于 `pomegranate`。

默认不记录 UserInfo 或 ID Token claims。仅在本地排错时，可临时设置 `OIDC_DEBUG_CLAIM_TYPES=true`；它只记录经过验证的 claim 名称和类型，不记录字段值。该开关还要求 `NODE_ENV=development`，验收完成后应恢复为 `false`。

## 平台用户映射

`platform_users.casdoor_subject` 保存经过 RS256 验签的 Casdoor `sub`，具有 `NOT NULL`、唯一和禁止空字符串约束。用户名不是稳定身份键，只会作为可更新资料保存。首次创建通过 PostgreSQL sequence 分配六位 `POME-` 编号；相同 subject 的并发首次登录使用单条 `INSERT ... ON CONFLICT DO UPDATE` 原子处理，最终只会保留一条记录。sequence 可能产生少量跳号，但不会通过“最大值加一”产生重复编号。

## 桌面登录 ticket

桌面端从 `/auth/login?client=desktop` 开始登录。OIDC callback 成功并完成平台用户映射后，Account Server 生成至少 32 字节密码学随机数的一次性 ticket，有效期 60 秒，并重定向到 `pomegranate://auth/callback?ticket=...`。自定义 URI 中只有 ticket，不包含 Casdoor token、subject、用户资料、平台 UUID或账号编号。

ticket 不是 access token，也不是长期会话。`POST /auth/desktop/exchange` 会原子消费它；相同 ticket 第二次交换、过期 ticket 和不存在的 ticket都会失败。交换成功时返回一个平台 session token 和最小用户资料，Rust 立即把 token 写入系统安全凭据存储，React 只接收用户资料。本地 ticket 使用带定时清理的进程内存储，只适合单实例 Account Server。生产环境部署多个 Account Server 实例前，必须改用支持原子消费和 TTL 的共享存储，例如 Redis 或数据库。任何日志都不得记录完整 ticket 或完整自定义 URI。

## 平台 session 与桌面恢复

平台 session token 由 32 字节密码学随机数生成，默认有效 7 天。数据库的 `user_sessions` 表只保存 token 的 SHA-256 哈希、所属平台用户、创建/过期/最后使用/撤销时间和可选设备标签，不保存原始 token。对高熵随机 token 使用 SHA-256 可以安全支持等值查找；它不是用户密码，因此不需要慢速密码派生算法。

- `GET /auth/session`：使用 `Authorization: Bearer ...` 查找未过期、未撤销的 session，更新 `last_used_at` 并返回最小用户资料。
- `POST /auth/logout`：只撤销当前 Bearer token 对应的 session，重复调用保持幂等，不影响同一账号的其他设备。
- Windows 桌面端：原始 token 保存到 Windows Credential Manager，服务名为 `cn.edu.pomegranate.account`；不写入 SQLite、localStorage、Zustand 持久化、JSON 或普通配置文件。
- 软件启动：Rust 读取系统凭据并请求 `/auth/session`。401 会清理失效凭据；临时网络错误会保留凭据并允许所有本地功能继续使用。
- 退出：Rust 先尝试调用 `/auth/logout`，无论网络撤销是否成功都会清除本机凭据；React 只清空用户资料，不会收到 token。

当前本地开发使用 HTTP 是因为 Account Server 只监听 `127.0.0.1`。未来跨机器部署必须启用 HTTPS，避免 Bearer session 在传输过程中泄露。

## 服务端个人文件空间

个人文件空间使用共享 PostgreSQL，但每条 `user_files` 记录都绑定 `owner_user_id`。该值只来自已经验证且未过期、未撤销的平台 session，客户端不能在请求中指定。列表、下载和删除 SQL 都同时匹配文件 ID 与当前 `platform_user_id`；访问其他用户的文件统一返回 404，不泄露文件是否存在。

PostgreSQL 只保存文件名、大小、SHA-256、所属用户和存储键等元数据。实际内容在本地开发环境保存到源码树之外的独立文件存储。磁盘文件名是 32 字节随机值生成的存储键，与原始文件名无关；路径由存储适配器根据安全根目录生成。写入采用同目录临时文件、同步写入和原子重命名；数据库写入失败会清理已经落盘的孤立文件，磁盘写入失败则不会创建数据库记录。

本地默认单文件上限为 20 MiB，由 `USER_FILE_MAX_BYTES=20971520` 控制。`USER_FILES_ROOT` 必须是源码目录之外的安全绝对路径，本机开发值为 `D:\PomegranateServer\data\user-files`。空文件允许上传，并正确保存 0 字节大小及空内容的 SHA-256。上传时报告的 MIME 只作为不可信元数据保存；下载统一使用 `application/octet-stream`、`attachment` 和 `nosniff`，服务不会执行、解析或公开托管上传内容。

普通上传采用扩展名白名单，并在 Rust 文件读取前和 Account Server 磁盘写入前各校验一次。当前允许 `doc/docx/xls/xlsx/csv/ppt/pptx/pdf`、`md/markdown/mdx/mdxl/txt/rtf/json/xml/yaml/yml` 以及 `png/jpg/jpeg/gif/webp/bmp/svg`；拒绝无扩展名、未知扩展名和 `exe/msi/dll/bat/cmd/com/scr/ps1/vbs/js/jse/jar/reg/lnk`。文件选择器过滤只用于改善体验，不是安全边界。MIME 不会覆盖扩展名策略，也不会让未知文件进入 WebView 执行。

## 统一文档目录（第一阶段）

`documents` 是 Markdown 与上传文件共用的服务端目录表。Markdown 正文保存在 PostgreSQL 的 `markdown_content`；Word、PDF 等文件的二进制内容保存在独立 `filesystem` 存储层，本地开发环境当前使用 `D:\PomegranateServer\data\user-files`。公开 API 不返回用户所有权字段、存储键、磁盘路径或 session。

`GET /documents`、`POST /documents/markdown`、`PATCH /documents/:documentId` 和 `DELETE /documents/:documentId` 都从当前 Bearer session 确定所有者。`POST /files` 成功后会同步创建一条 `uploaded_file` 文档；删除文件会同时软删除两侧目录记录并清理磁盘内容。`POST /documents/import-local-markdown` 仅供受认证的本地迁移工具使用，客户端不能指定目标用户。

桌面端的一次性迁移模块先从 Windows Credential Manager 读取 session，并强制确认账号编号为 `POME-000001`。它使用 SQLite `VACUUM INTO` 在应用数据目录下的 `account-document-backups` 创建时间戳备份，之后只读复制 Markdown 与旧字段元数据；不会删除、清空或修改原 SQLite。重复导入以 `(owner_user_id, source_local_document_id)` 唯一约束保持同一文档 ID。

账号模式下，React“文档”页面使用统一 `documents` API。页面中的“上传文件”保留源文件字节并创建 `uploaded_file`；单独的“导入为可编辑 Markdown”只接受 UTF-8 的 `.md/.markdown`，将正文写入 PostgreSQL，不创建 `user_files` 或文件存储副本。未来部署到云端时，只同步代码和 migration，不复制本地实时数据库；PostgreSQL 与文件存储都必须采用持久化、备份和最小权限配置。

## 独立文件存储

账号系统由两类功能程序和两类数据存储组成：Casdoor 负责注册与认证，Account Server 负责平台账号、文档和文件业务；PostgreSQL 保存结构化数据，独立文件存储保存上传的二进制内容。Markdown 与日记正文适合事务、版本和查询，因此保存在 PostgreSQL；Word、PDF、PPT、Excel 和图片等内容体积较大，保存在文件存储。`documents` 通过 `kind` 和关联记录把两类内容统一呈现为同一个用户文档池。

本机开发存储根目录为 `D:\PomegranateServer\data\user-files`。磁盘寻址只使用数据库中的 `storage_key`，不使用原始文件名。`FILE_STORAGE_BACKEND` 当前只接受 `filesystem`；未来 Linux 可配置为 `/srv/pomegranate/user-files`，也可在实现新的存储适配器后迁移到 S3/MinIO。

旧的 `services/account-server/.data/user-files` 迁移后只作为回滚备份，不再接受新写入。迁移必须先停止 Account Server，再依次执行备份、`storage:migrate -- --dry-run`、`storage:migrate -- --copy` 和 `storage:verify`。工具按 `user_files` 记录复制，逐个核验大小与 SHA-256，不修改数据库记录；相同目标安全跳过，冲突目标停止且绝不覆盖。PowerShell 备份工具位于 `scripts/account-storage`。

如果新存储切换失败，先停止 Account Server。在 `NODE_ENV=development` 下，可暂时把 `USER_FILES_ROOT` 指回旧目录并设置 `FILE_STORAGE_ALLOW_LEGACY_ROLLBACK=true` 后启动；恢复后应尽快修复新目录并取消该开关。该开关仅允许精确的历史目录，默认关闭，生产环境不可用。

云端迁移必须同时迁移 PostgreSQL 数据和文件存储内容；只迁 PostgreSQL 会留下可见的文件记录，却缺少实际内容。本地 DEV 与云端只同步代码和 SQL migration，不进行实时双向数据同步。

四个 API 都要求 `Authorization: Bearer <sessionToken>`：

- `POST /files`：`multipart/form-data`，唯一文件字段必须名为 `file`，每次只接受一个文件。
- `GET /files?limit=50&offset=0`：只列出当前用户未删除记录，`limit` 最大 100。
- `GET /files/:fileId/download`：只下载当前用户未删除文件。
- `DELETE /files/:fileId`：设置 `deleted_at` 后清理磁盘内容；重复删除返回 404。

删除后数据库仍保留软删除元数据用于审计，但本地磁盘内容会被删除且不可恢复。当前没有课堂共享、跨用户授权、病毒扫描、文件签名嗅探、用户配额、对象存储、备份或恢复机制。生产部署前必须加入恶意文件扫描、真实类型与 MIME/签名一致性策略、用户与组织配额、备份，并把内容迁移到具有私有访问控制的 S3/MinIO 等对象存储；还需要处理数据库与对象存储之间失败补偿和定期孤立对象清理。

## 安装与检查

只安装 Account Server 自身依赖：

```powershell
pnpm --filter @pomegranate/account-server install
pnpm --filter @pomegranate/account-server typecheck
pnpm --filter @pomegranate/account-server test
```

## 执行 migration

确保本地 PostgreSQL 已运行且服务级 `.env` 已填写，然后执行：

```powershell
pnpm --filter @pomegranate/account-server migrate
```

migration 在事务中执行。重复执行时，已成功应用且内容未改变的文件会安全跳过；若已应用文件的校验和发生变化，命令会失败并要求新增 migration，而不是覆盖已有结构。

## 启动和健康检查

启动开发服务：

```powershell
pnpm --filter @pomegranate/account-server dev
```

另开一个终端检查：

```powershell
Invoke-WebRequest http://127.0.0.1:3010/health/live
Invoke-WebRequest http://127.0.0.1:3010/health/ready
```

- `/health/live` 只证明服务进程存活，不访问数据库。
- `/health/ready` 通过 `SELECT 1` 检查 PostgreSQL；可用时返回 200，不可用时返回 503。

## uploaded_file 外部编辑工作副本

`PUT /files/:fileId/content` 只在当前 Session 有效、文件属于当前平台用户且
`expectedSha256` 与服务端当前哈希完全一致时替换内容。服务端先写完并校验一个新的
不可变存储对象，再在同一 PostgreSQL 事务中切换 `user_files` 元数据并递增关联
`documents.revision`。旧哈希会返回 `409 file_conflict`，没有强制覆盖入口；旧存储对象
只在事务成功提交后清理。

桌面端实际编辑的是应用数据目录下
`account-document-workspaces/<平台用户安全哈希>/<文档文件安全哈希>/` 中的账号隔离副本。
工作副本不是服务器真源。用户选择“暂不保存”时不会删除尚未同步的副本，以免外部编辑
内容静默丢失；React 不会获得真实绝对路径、Session 或存储键。
