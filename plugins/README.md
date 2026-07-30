# 插件源码归档入口

这里用于保存可开发、可审核、可打包分发的插件源码包和示例插件。

运行时安装插件不放在这里。桌面端运行时插件目录由应用数据目录管理：

```text
<data_dir>/plugins/<plugin-id>
```

插件迁移规则：

- 插件源码包保留 manifest、资源、前端扩展声明和最小运行依赖。
- 插件默认不授权；权限由 manifest 声明并由用户或云端策略授予。
- 插件调用本地能力必须经过 Rust `plugin_proxy_*` 命令。
- 插件调用云端能力必须经过账号服务代理，不能自行传 `owner_user_id`、`student_id` 或密钥。
- 插件市场、审核、版本和授权策略归云端；插件执行归桌面本地。

具体边界见 `docs/summary3-cloud-local-archive.md` 和 `docs/account-classroom-isolation.md`。
