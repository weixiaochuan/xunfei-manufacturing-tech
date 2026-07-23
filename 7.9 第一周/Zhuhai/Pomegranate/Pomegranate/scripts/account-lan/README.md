# Pomegranate 局域网测试环境

这套脚本仅用于同一可信局域网内的临时联调。它不会部署公网、域名、HTTPS，也不会向第二台电脑安装 Docker、Node、PostgreSQL 或 Casdoor。PostgreSQL 始终只绑定 `127.0.0.1:5432`。

## 主机准备与启动

1. 在 Windows 设置中确认当前 Wi-Fi/以太网是“专用网络”。公用网络会被脚本拒绝。
2. 在普通 PowerShell 运行 `scripts/account-lan/prepare-lan.ps1`。已有 `.env.account.lan` 不会被覆盖，除非显式使用 `-Force`。
3. 用管理员 PowerShell 运行 `scripts/account-lan/enable-lan-firewall.ps1`，它只为当前物理网卡、Private profile 和当前子网开放 TCP 3010/8000。
4. 在 Casdoor `pomegranate/app-pomegranate` 的 Redirect URLs 中保留本机回调，并增加 `http://<LAN_IP>:3010/auth/callback`。不要改桌面 deep link `pomegranate://auth/callback`。此项为 **MANUAL**。
5. 运行 `scripts/account-lan/start-account-lan.ps1`，然后运行 `scripts/account-lan/verify-account-lan.ps1`。

停止 Account Server：`scripts/account-lan/stop-account-lan.ps1`。只有需要一并暂停数据服务时才添加 `-StopDataServices`；脚本从不执行 `down -v`。

## 第二台 Windows 电脑

两台电脑连接同一 Wi-Fi/路由器/热点后，先在浏览器访问 `http://<LAN_IP>:3010/health/live` 和 `http://<LAN_IP>:8000/.well-known/openid-configuration`，再运行 `Test-NetConnection <LAN_IP> -Port 3010` 与 `-Port 8000`。两项应为 True；5432 应不可达。然后安装 LAN TEST NSIS 包并确认 `pomegranate://` 已注册。

第二台电脑只运行 Pomegranate。登录 Session 保存在 Windows Credential Manager；账号隔离缓存仍按既有实现处理。注册、跨设备文档/文件、重启恢复及账号隔离需要在 GUI 中人工验收并标记为 **MANUAL**。

## 排错

- 检查当前网络是否为 Private，切勿关闭整个 Windows 防火墙。
- 检查 Account Server 是否监听 `0.0.0.0:3010`，Casdoor 是否绑定选定 LAN IP 的 8000。
- 运行 `check-lan-firewall.ps1`，确认规则只限 Private/当前子网且没有 5432。
- 若同网段仍不通，检查 AP/Client Isolation、VPN、代理、校园网策略或手机热点设备互访限制。
- Discovery 的 issuer、authorization、token、userinfo、JWKS 地址必须全部是 LAN IP，不能出现 localhost。

## 未来迁移云端

只需替换 `DEPLOYMENT_PROFILE`、`ACCOUNT_SERVER_PUBLIC_URL`、`CASDOOR_PUBLIC_URL`、`CASDOOR_REDIRECT_URI`、数据库连接、文件根目录和客户端公开 Account Server URL，并把 HTTP 改为 HTTPS。代码与 migration 可以同步，数据库数据不能做本地/云端实时双向同步；正式迁移应采用备份、导入和校验。云端由反向代理只开放 80/443（以及受限管理用 22），不得直接开放 5432、8000、3010。
