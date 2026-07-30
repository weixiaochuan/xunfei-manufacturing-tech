// 此文件由 scripts/plugin-capabilities.mjs 生成，请勿手工修改。
export const PLUGIN_CAPABILITY_IDS = [
  "notes.read",
  "notes.write",
  "document.read",
  "document.write",
  "tasks.read",
  "tasks.write",
  "ai.invoke",
  "ai.context.read",
  "ai.context.augment",
  "ai.session.read",
  "ui.editor.toolbar",
  "ui.chat.toolbar",
  "ui.chat.panel",
  "planning.files.read",
  "planning.files.write",
  "network.request",
  "files.readSelected",
  "files.writeSelected",
  "prompts.register",
  "views.register",
  "mcp.connect",
  "credentials.use",
  "credentials.configure",
  "network.xingchen",
  "agents.invoke",
  "editor:read",
  "editor:write",
  "workspace:read",
  "workspace:write",
  "notes:read",
  "notes:write",
  "settings:read",
  "settings:write",
  "files:read",
  "files:write",
  "network:request",
  "clipboard:read",
  "clipboard:write",
  "tasks.subscribe",
  "taskViews.register",
  "ai:chat",
  "ai:models"
] as const;
export type PluginCapabilityId = (typeof PLUGIN_CAPABILITY_IDS)[number];
export const PLUGIN_V3_CAPABILITY_IDS = [
  "document.read",
  "document.write",
  "tasks.read",
  "tasks.write",
  "ai.invoke",
  "ai.context.read",
  "ai.context.augment",
  "ai.session.read",
  "ui.editor.toolbar",
  "ui.chat.toolbar",
  "ui.chat.panel",
  "planning.files.read",
  "planning.files.write",
  "network.request",
  "files.writeSelected",
  "prompts.register",
  "mcp.connect",
  "credentials.use",
  "network.xingchen",
  "agents.invoke"
] as const;
export type PluginV3CapabilityId = (typeof PLUGIN_V3_CAPABILITY_IDS)[number];
export const PLUGIN_CAPABILITY_PRESENTATION = {
  "notes.read": {
    "title": "读取笔记",
    "description": "读取宿主管理的笔记；点式 v3 宿主门禁尚未接入。",
    "riskLevel": "high",
    "status": "reserved"
  },
  "notes.write": {
    "title": "修改笔记",
    "description": "修改宿主管理的笔记；点式 v3 宿主门禁尚未接入。",
    "riskLevel": "critical",
    "status": "reserved"
  },
  "document.read": {
    "title": "读取当前文档",
    "description": "读取当前文档内容；当前仅文档摘要宿主路径实施门禁。",
    "riskLevel": "high",
    "status": "restricted"
  },
  "document.write": {
    "title": "写入当前文档",
    "description": "向当前文档写入插件结果；当前仅文档摘要宿主路径实施门禁。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "tasks.read": {
    "title": "读取待办",
    "description": "通过受控插件代理读取待办。",
    "riskLevel": "medium",
    "status": "restricted"
  },
  "tasks.write": {
    "title": "修改待办",
    "description": "通过受控插件代理创建、更新、完成或删除待办。",
    "riskLevel": "high",
    "status": "restricted"
  },
  "ai.invoke": {
    "title": "调用 AI",
    "description": "通过宿主管理的摘要或受控智能体工作流调用 AI。",
    "riskLevel": "high",
    "status": "restricted"
  },
  "ai.context.read": {
    "title": "读取 AI 上下文",
    "description": "读取宿主管理的 AI 会话上下文；当前仅 Planning 插件路径实施门禁。",
    "riskLevel": "high",
    "status": "restricted"
  },
  "ai.context.augment": {
    "title": "增强 AI 调用上下文",
    "description": "在模型调用前后注入声明式上下文资源。",
    "riskLevel": "high",
    "status": "restricted"
  },
  "ai.session.read": {
    "title": "读取 AI 会话",
    "description": "读取当前 AI 会话状态；当前仅 Planning 插件路径实施门禁。",
    "riskLevel": "high",
    "status": "restricted"
  },
  "ui.editor.toolbar": {
    "title": "注册编辑器工具栏按钮",
    "description": "向编辑器工具栏注册声明式插件入口。",
    "riskLevel": "medium",
    "status": "restricted"
  },
  "ui.chat.toolbar": {
    "title": "注册聊天工具栏入口",
    "description": "向聊天工具栏注册宿主管理的插件入口；当前仅 Planning 插件路径实施门禁。",
    "riskLevel": "medium",
    "status": "restricted"
  },
  "ui.chat.panel": {
    "title": "注册聊天面板",
    "description": "向聊天界面注册宿主管理的插件面板；当前仅 Planning 插件路径实施门禁。",
    "riskLevel": "medium",
    "status": "restricted"
  },
  "planning.files.read": {
    "title": "读取规划文件",
    "description": "读取 Planning 插件受控工作区中的规划文件。",
    "riskLevel": "high",
    "status": "restricted"
  },
  "planning.files.write": {
    "title": "写入规划文件",
    "description": "写入 Planning 插件受控工作区中的规划文件。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "network.request": {
    "title": "受控网络请求",
    "description": "通过 Marketplace 服务配置访问已验证的远程端点。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "files.readSelected": {
    "title": "读取用户选择文件",
    "description": "读取用户显式选择的文件；宿主 capability 尚未接入。",
    "riskLevel": "high",
    "status": "reserved"
  },
  "files.writeSelected": {
    "title": "写入用户选择位置",
    "description": "允许受控 Feature 返回并保存用户确认的文件输出。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "prompts.register": {
    "title": "注册 Prompt",
    "description": "安装由 Marketplace 管理的 Prompt 模板；逐次权限门禁尚不完整。",
    "riskLevel": "medium",
    "status": "restricted"
  },
  "views.register": {
    "title": "注册视图",
    "description": "注册正式插件视图；统一宿主门禁尚未接入。",
    "riskLevel": "medium",
    "status": "reserved"
  },
  "mcp.connect": {
    "title": "连接 MCP",
    "description": "配置 Marketplace 管理的远程 MCP 连接；当前仅创建禁用的 mock 注册项。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "credentials.use": {
    "title": "使用凭据引用",
    "description": "允许宿主使用已绑定凭据 ID，插件不获得凭据明文。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "credentials.configure": {
    "title": "配置凭据",
    "description": "插件直接配置凭据的能力被禁止；凭据只能由宿主账号界面管理。",
    "riskLevel": "critical",
    "status": "blocked"
  },
  "network.xingchen": {
    "title": "访问讯飞星辰服务",
    "description": "通过宿主受控的星辰 Agent/Workflow 服务发起请求。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "agents.invoke": {
    "title": "调用已绑定智能体",
    "description": "通过宿主调用用户已绑定的智能体或工作流。",
    "riskLevel": "critical",
    "status": "restricted"
  },
  "editor:read": {
    "title": "读取编辑器（旧版）",
    "description": "旧版 WebView 插件读取编辑器选择内容；未形成独立安全边界。",
    "riskLevel": "high",
    "status": "legacy"
  },
  "editor:write": {
    "title": "修改编辑器（旧版）",
    "description": "旧版 WebView 插件注册编辑器交互；调用级权限门禁不完整。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "workspace:read": {
    "title": "读取工作区（旧版）",
    "description": "旧版工作区读取名称；实际能力复用笔记代理。",
    "riskLevel": "high",
    "status": "legacy"
  },
  "workspace:write": {
    "title": "修改工作区（旧版）",
    "description": "旧版工作区写入权限名称；未发现独立宿主执行路径。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "notes:read": {
    "title": "读取笔记（旧版）",
    "description": "旧版插件通过 token 化代理读取笔记。",
    "riskLevel": "high",
    "status": "legacy"
  },
  "notes:write": {
    "title": "修改笔记（旧版）",
    "description": "旧版插件通过 token 化代理创建、更新或删除笔记。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "settings:read": {
    "title": "读取设置（旧版）",
    "description": "旧版插件读取自身设置；当前代理只检查 token。",
    "riskLevel": "medium",
    "status": "legacy"
  },
  "settings:write": {
    "title": "修改设置（旧版）",
    "description": "旧版插件修改自身设置；当前代理只检查 token。",
    "riskLevel": "high",
    "status": "legacy"
  },
  "files:read": {
    "title": "读取文件（旧版）",
    "description": "旧版宽范围文件读取权限名称；未发现受控宿主代理。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "files:write": {
    "title": "写入文件（旧版）",
    "description": "旧版宽范围文件写入权限名称；未发现受控宿主代理。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "network:request": {
    "title": "网络请求（旧版）",
    "description": "旧版网络权限名称；未发现受控宿主网络代理。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "clipboard:read": {
    "title": "读取剪贴板（旧版）",
    "description": "旧版剪贴板读取权限名称；未发现插件级受控代理。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "clipboard:write": {
    "title": "写入剪贴板（旧版）",
    "description": "旧版剪贴板写入权限名称；未发现插件级受控代理。",
    "riskLevel": "high",
    "status": "legacy"
  },
  "tasks.subscribe": {
    "title": "订阅待办事件（旧版）",
    "description": "旧版插件订阅待办事件；当前只有前端提示，没有后端权限门禁。",
    "riskLevel": "high",
    "status": "legacy"
  },
  "taskViews.register": {
    "title": "注册待办视图（旧版）",
    "description": "旧版插件注册待办视图；当前只有前端提示，没有后端权限门禁。",
    "riskLevel": "medium",
    "status": "legacy"
  },
  "ai:chat": {
    "title": "调用 AI 对话（旧版）",
    "description": "旧版插件通过 token 化代理调用 AI 对话。",
    "riskLevel": "critical",
    "status": "legacy"
  },
  "ai:models": {
    "title": "读取 AI 模型列表（旧版）",
    "description": "旧版插件通过 token 化代理读取模型元数据。",
    "riskLevel": "high",
    "status": "legacy"
  }
} as const;
