# Plugin Registry Cloud Module

这里是插件市场、审核中心和授权策略的云端归档入口。

该模块负责：

- 插件包版本、签名或内容哈希。
- 插件审核状态。
- 插件适用场景和能力声明。
- 账号、组织、班级维度的插件授权策略。
- 插件下发和撤回。

该模块不负责执行插件。插件执行属于桌面端本地运行时，当前入口包括：

- `src-tauri/src/services/plugins.rs`
- `src-tauri/src/commands/plugin_proxy.rs`
- `src/services/pluginManager.ts`
- `plugins/`

安全规则：

- 云端负责分发和授权，桌面端负责令牌、权限校验和代理执行。
- 插件不能拿到账号 session token、Casdoor token 或 AI provider key。
- 插件产生的云端数据必须由账号服务代理写入当前账号或被授权班级。

详细约束见 `docs/summary3-cloud-local-archive.md`。
