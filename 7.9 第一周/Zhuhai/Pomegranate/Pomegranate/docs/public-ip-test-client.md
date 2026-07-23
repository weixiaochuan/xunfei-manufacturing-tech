# Pomegranate Public IP TEST 构建前准备

## 用途与边界

Public IP TEST 仅用于 ICP 备案完成前的短期异地联调。Account Server 和 Casdoor
公网地址都由构建命令显式传入，客户端安装完成后不能修改连接目标。当前没有确认服务器
公网端口，因此本轮只准备配置与脚本，不生成 EXE、NSIS、manifest 或 SHA-256 文件。

正式域名版 Cloud TEST 已单独保留，并继续固定连接
`https://api.stargathering.cn`。两种客户端业务功能相同，主要区别是连接目标以及临时
环境可能使用公网 HTTP。

## 安全限制

`public-ip-test` profile 只接受带明确端口的公网 IPv4 HTTP(S) 根地址，拒绝空值、
localhost、回环地址、未指定地址、局域网、链路本地、共享地址、基准测试地址、
文档示例保留地址、组播、保留地址、广播地址、URL 凭据、路径、查询参数和 fragment。
HTTP 还必须在构建时显式传入 `-AllowInsecureHttp`；否则参数验证立即失败。

使用公网 HTTP 时必须遵守：

- 只使用临时测试账号和全新的临时密码。
- 不复用任何真实账号密码。
- 不上传隐私数据、正式课程材料或真实业务文件。
- 不迁移真实数据，也不得提供给正式用户。
- ICP 与 HTTPS 生效后立即关闭公网 HTTP 入口并切换到正式域名版。

## 版本并存与更新

Public IP TEST 继续沿用现有 identifier、`pomegranate` Deep Link 和 Windows
Credential Manager 凭据槽。因此 Public IP TEST、Cloud TEST、正式版与 LAN TEST
不能安全并存。安装前需要完全关闭并卸载旧安装版本。

Public IP TEST 的 updater endpoints 为空，也不生成 updater artifacts。测试期间通过
人工发送新的安装包更新。

## 服务器同学必须提供的信息

最终构建前必须明确提供并核验：

1. Account Server 公网完整地址，包括协议、IPv4 和端口。
2. Casdoor 公网完整地址，包括协议、IPv4 和端口。
3. Account Server 回调地址确认，必须等于 `<ApiBaseUrl>/auth/callback`。
4. Account Server `/health/live` 实际结果。
5. Account Server `/health/ready` 实际结果。
6. Casdoor `/.well-known/openid-configuration` 实际结果。

Auth URL 不是桌面客户端运行时连接目标。客户端只连接 Account Server，
Account Server 再跳转到 Casdoor；Auth URL 仅用于构建参数校验、manifest 和联调说明。

## 参数验证

以下命令使用占位符，必须由服务器同学提供真实值后替换：

```powershell
powershell -ExecutionPolicy Bypass -File `
  .\scripts\cloud-client\build-public-ip-test.ps1 `
  -ApiBaseUrl "http://<PUBLIC_IP>:<ACCOUNT_SERVER_PORT>" `
  -AuthBaseUrl "http://<PUBLIC_IP>:<CASDOOR_PORT>" `
  -AllowInsecureHttp `
  -ValidateOnly
```

`-ValidateOnly` 只验证参数和 Tauri 配置，不编译 Rust、不构建前端、不生成 EXE、
NSIS、manifest 或 SHA-256 文件，也不会连接输入的服务器。

## 未来一键构建

服务器地址和健康检查确认后执行：

```powershell
powershell -ExecutionPolicy Bypass -File `
  .\scripts\cloud-client\build-public-ip-test.ps1 `
  -ApiBaseUrl "http://<PUBLIC_IP>:<ACCOUNT_SERVER_PORT>" `
  -AuthBaseUrl "http://<PUBLIC_IP>:<CASDOOR_PORT>" `
  -AllowInsecureHttp `
  -OutputDirectory "D:\PomegranateBuilds\public-ip-test\<DATE>"
```

如果两个服务都已启用 HTTPS，去掉 `-AllowInsecureHttp`。脚本仅在当前构建进程中设置：

- `POMEGRANATE_DEPLOYMENT_PROFILE=public-ip-test`
- `POMEGRANATE_ACCOUNT_SERVER_URL=<ApiBaseUrl>`
- `POMEGRANATE_ALLOW_INSECURE_PUBLIC_IP_HTTP=true|false`
- `VITE_DOCUMENT_SOURCE=account`

构建完成后脚本恢复原有进程环境变量，不修改用户或系统全局环境变量。未来公开 manifest
记录 API、Auth、回调、部署 profile、是否使用临时不安全传输、安装包信息、签名状态和
`WAITING_FOR_SERVER`，但绝不包含 Client Secret、数据库密码、token、用户文件或服务器
秘密。
