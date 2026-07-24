# Pomegranate Cloud TEST Windows 客户端

## 用途与固定公开地址

本配置只用于 Windows x64 Cloud TEST 客户端。构建分支为
`cloud-test-client-20260723`，固定公开地址如下：

- Account Server：`https://api.stargathering.cn`
- Casdoor 认证服务：`https://auth.stargathering.cn`
- 桌面回调：`pomegranate://auth/callback`

客户端不直接调用 Casdoor。点击登录后，Rust 只打开 Account Server 的
`/auth/login?client=desktop`；Account Server 再跳转到认证服务，完成认证后通过
`pomegranate://auth/callback` 唤醒桌面程序。授权码、Casdoor Client Secret 和令牌
交换都留在服务端，React 不接收桌面 session token。

## 构建

在项目根目录安装锁定依赖并准备 MCP sidecar 后执行：

```powershell
pnpm install --frozen-lockfile
pnpm build:mcp:debug
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\cloud-client\build-cloud-test.ps1
```

默认输出目录是 `D:\PomegranateBuilds\cloud-test\20260723`。如需更改：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\cloud-client\build-cloud-test.ps1 `
  -OutputDirectory 'D:\PomegranateBuilds\cloud-test\20260723'
```

构建脚本只在当前进程中设置：

- `POMEGRANATE_DEPLOYMENT_PROFILE=cloud`
- `POMEGRANATE_ACCOUNT_SERVER_URL=https://api.stargathering.cn`
- `VITE_DOCUMENT_SOURCE=account`

脚本结束后会恢复这些变量原有的进程值，不修改用户或系统全局环境变量。Cloud
profile 只接受固定 HTTPS API 主机，拒绝 HTTP、localhost、回环地址、局域网 IP、
凭据、显式端口和额外路径。文档来源固定为 Account Server，不回退到 SQLite。

## 安装、并存与更新

Cloud TEST 保留稳定的 `edu.bit.inb` identifier、Windows Credential Manager 项和
`pomegranate` deep-link scheme。这样不会改造既有认证链路，但它与正式版使用相同
安装标识、协议和安全凭据槽，不能作为可安全并存的独立产品。安装 Cloud TEST 前，
请先完全关闭并卸载重要的旧安装版本；不要同时运行正式版、LAN TEST 和 Cloud TEST。

Cloud TEST 配置将 updater endpoints 置空，也不生成 updater artifacts，因此不会从
旧 Gitee 渠道自动检查或安装不兼容版本。当前通过新的 Cloud TEST 安装包人工更新。
此隔离只作用于 Cloud TEST，不删除正式产品未来的 updater 能力。

安装包目前不带 Windows 代码签名。SmartScreen 可能显示未知发布者；只应从项目负责
人确认过 SHA-256 的渠道获取，并在核对哈希后决定是否继续。卸载使用 Windows
“设置 → 应用 → 已安装的应用”，选择 Pomegranate Cloud TEST。

应用保留现有外部编辑能力。编辑 DOCX/PPTX 需要本机安装可用的 WPS Office 或
Microsoft Office；具体关联行为取决于 Windows 默认应用设置。

## SHA-256 校验

输出目录同时包含 `.sha256` 文件和 `cloud-test-build-manifest.json`。可重新计算：

```powershell
Get-FileHash `
  -LiteralPath 'D:\PomegranateBuilds\cloud-test\20260723\Pomegranate Cloud TEST_1.8.0_x64-setup.exe' `
  -Algorithm SHA256
```

结果应与 `.sha256` 文件及 manifest 中的 `installerSha256` 完全一致。manifest 的
`serverValidationStatus` 在服务器上线前必须保持 `WAITING_FOR_SERVER`。

## 安装包绝不包含的内容

以下公开地址可以进入配置、文档或安装包：

- `https://api.stargathering.cn`
- `https://auth.stargathering.cn`
- `pomegranate://auth/callback`

安装包不得包含 Casdoor Client Secret、数据库密码、Casdoor 管理员密码、SSH 私钥、
HTTPS 私钥、生产 `.env.cloud`、用户文件、session token、`storage_key` 或服务器绝对
路径。构建脚本不会读取生产环境文件、连接 PostgreSQL、修改 Casdoor 或访问本机
Account Server。

## 服务器上线后的人工验收

当前所有公网验证均为 `WAITING_FOR_SERVER`。发送给测试同学前，至少先确认：

1. `https://api.stargathering.cn/health/live`
2. `https://api.stargathering.cn/health/ready`
3. `https://auth.stargathering.cn/.well-known/openid-configuration`

三项通过后，在没有重要旧版本的 Windows 电脑执行：

1. 安装并启动 Cloud TEST。
2. 点击登录，确认浏览器进入 `auth.stargathering.cn`。
3. 登录后确认 deep link 返回 Pomegranate。
4. 新建 Markdown，上传 TXT/DOCX，并在第二台电脑验证同账号同步。
5. 使用不同账号验证隔离。
6. 用 WPS/Office 外部编辑并验证同步及文件 `409` 冲突处理。

浏览器登录、注册、跨电脑文档、上传下载和外部编辑同步在云端尚未部署时都不能声称
通过。
