# Manifest v3 受控插件示例

本目录提供三个不含真实凭据的源码示例：

- `v3-declarative-writer/`：`feature`，由 firstwork 渲染声明式表单并展示本地演示结果。
- `v3-learning-research-enhancer/`：`enhancement`，增强 AI 助学的目标理解和计划生成提示上下文。
- `v3-xingchen-workflow-feature/`：`feature`，通过 AI 资源中心已配置的讯飞 Workflow 执行真实后端调用。

在 `Pomegranate` 目录执行：

```powershell
pnpm.cmd plugin:pack -- dev-plugins\v3-declarative-writer
pnpm.cmd plugin:pack -- dev-plugins\v3-learning-research-enhancer
pnpm.cmd plugin:pack -- dev-plugins\v3-xingchen-workflow-feature
```

生成的 `.firstwork-plugin` 是本地构建产物，不随源码提交。安装前应在插件管理页检查
Manifest、权限变化、签名状态和完整性摘要。

## 星辰示例的使用条件

1. 先在 AI 资源中心保存用户自己的讯飞凭据并创建可用 Workflow 智能体。
2. 安装时确认 `credentials.use`、`agents.invoke`、`network.xingchen` 和 `ai.invoke` 权限。
3. 打开插件功能页后选择智能体，再填写声明式表单。

插件包只声明表单和所需能力，不保存 `API Key`、`API Secret`、Token 或 Endpoint 鉴权头。
请求由 Rust 后端复用现有 `XingchenAgentService` 发出，可能把表单数据发送至讯飞并消耗用户额度。

## 安全限制

- 正式包只允许 `declarative-ui`、`prompt-pack` 和受控星辰运行时。
- `feature` 必须提供 `uiSchema`，handler 只能为 `declarative`。
- 包内 JavaScript、Python、Shell、可执行文件、`.env` 和疑似密钥会被预检拒绝。
- 插件不能获得通用 Tauri `invoke`，也不能读取安全凭据明文。
- `legacy-js` 兼容能力不属于正式市场插件路径。
