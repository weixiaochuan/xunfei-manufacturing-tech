# 番茄钟插件 (Pomodoro)

> 示范插件 — 验证 PluginAppAPI 完整的任务读写 + 事件订阅 + 视图注册链路。

## 功能

| 功能 | 实现方式 | 验证的 API |
|------|---------|-----------|
| 自动开始番茄钟 | 监听 `task:created` 事件 | `app.events.on('task:created')` |
| 25 分钟倒计时 | `setTimeout` 写入描述 | `app.tasks.get()` / `app.tasks.update()` |
| 番茄记录 | 任务描述追加 `🍅 时间戳` | `app.tasks.update(id, { description })` |
| 命令面板集成 | 注册 `pomodoro.start` 命令 | `app.commands.addCommand()` |
| 今日番茄视图 | 任务页新增"今日番茄"标签 | `app.taskViews.register()` |

## 安装

1. 将 `dev-plugins/pomodoro/` 复制到插件的安装目录
2. 在应用「设置 → 插件管理」中启用
3. 授权 `tasks.subscribe` + `tasks.write` 权限

## 使用

- **创建任务** → 自动弹出"开始番茄钟"提示并启动 25 分钟倒计时
- **25 分钟后** → 任务描述自动追加 🍅 记录
- **命令面板** (`Ctrl+Shift+P`) → 搜索"启动番茄钟"
- **任务页视图切换** → 选择"今日番茄"查看统计

## 权限

```json
{
  "permissions": ["tasks.subscribe", "tasks.write"]
}
```

## 本地开发

```bash
# 直接复制到 dev-plugins 目录即可
cp -r pomodoro/ <app-data>/dev-plugins/pomodoro/
```
