# 会话窗 Tab 化方案

## 目标

在插件窗口中以浏览器式 Tab 管理不同项目文件夹的会话。每个 Tab 对应一个项目目录，切换 Tab 即切换该项目的 AI 会话上下文、项目状态和插件任务视图。

第一版采用推荐方案 A：顶部 Tab + 项目信息栏 + 左侧上下文侧栏 + 主会话区。

## 核心原则

| 原则 | 说明 |
|------|------|
| 项目即会话容器 | 一个项目文件夹对应一个 ProjectSession |
| Tab 管理打开状态 | Tab 只表示当前窗口打开的项目会话，不等于删除历史会话 |
| 数据持久化在 Rust 侧 | 项目会话、消息、上下文通过 SQLite 保存 |
| UI 状态在 Zustand | 当前激活 Tab、加载状态、局部视图状态放前端 |
| 文件系统能力收口到 Rust | 路径校验、Git 信息、会话恢复由 Rust Command 处理 |
| 第一版保持轻量 | 先完成多项目 Tab 容器，不做终端、拖拽排序、多窗口 |

## 页面布局

```text
┌────────────────────────────────────────────────────────────┐
│ + 打开项目   [W-NoteBook ●] [Plugin Demo ○] [Docs Site ⚠] │
├────────────────────────────────────────────────────────────┤
│ 当前项目: D:\AI\W-NoteBook     Git: master     状态: 活跃   │
├───────────────┬────────────────────────────────────────────┤
│ 会话上下文侧栏 │ 主会话区域                                  │
│               │                                            │
│ 当前任务       │ 消息列表 / Agent 工作记录                    │
│ 最近文件       │                                            │
│ Git 状态       │ 输入框 + 工具按钮                            │
│ 插件工具       │                                            │
└───────────────┴────────────────────────────────────────────┘
```

## UI 区域设计

### 顶部 ProjectTabBar

负责展示已打开项目会话。

包含：

- `+ 打开项目` 按钮
- 项目 Tab 列表
- 当前激活项目高亮
- Tab 关闭按钮
- 项目过多时的更多菜单

Tab 显示建议：

```text
[项目名 状态点 关闭]
```

状态点含义：

| 状态 | 含义 |
|------|------|
| 灰色 | 未加载 |
| 蓝色 | 当前活跃 |
| 绿色 | 会话正常 |
| 黄色 | 有未保存上下文或任务运行中 |
| 红色 | 会话异常 |

### ProjectSessionHeader

展示当前激活项目的摘要信息。

字段：

- 项目名称
- 项目路径
- Git 分支
- Git 工作区摘要
- 会话状态
- 最近活跃时间

示例：

```text
W-NoteBook    D:\AI\W-NoteBook    master    12 changed    active
```

### ProjectContextSidebar

左侧上下文栏展示当前项目会话的关键上下文。

推荐卡片：

| 卡片 | 内容 |
|------|------|
| 当前任务 | 当前会话正在处理的任务标题、状态 |
| 最近文件 | 最近访问或 AI 引用的文件 |
| Git 状态 | 分支、变更数量、未跟踪数量 |
| 插件工具 | 当前项目可用插件入口 |

### ProjectSessionMain

主会话区域。

第一版包含：

- 消息列表
- 空状态提示
- 输入框
- 发送按钮
- 基础工具按钮

后续可扩展：

- Agent 活动流
- 文件引用面板
- 计划侧栏
- 任务运行日志

## 组件拆分

```text
ProjectSessionWindow
├── ProjectTabBar
│   ├── AddProjectButton
│   ├── ProjectTabItem
│   └── ProjectOverflowMenu
├── ProjectSessionHeader
├── ProjectSessionLayout
│   ├── ProjectContextSidebar
│   │   ├── CurrentTaskCard
│   │   ├── RecentFilesList
│   │   ├── GitStatusSummary
│   │   └── PluginToolsList
│   └── ProjectSessionMain
│       ├── SessionMessageList
│       ├── AgentActivityPanel
│       └── SessionInputBar
```

Ant Design 组件建议：

| 场景 | 组件 |
|------|------|
| Tab 管理 | `Tabs` |
| 项目操作 | `Button`, `Dropdown` |
| 状态展示 | `Tag`, `Badge` |
| 信息卡片 | `Card`, `List` |
| 关闭确认 | `Modal.confirm` |
| 空状态 | `Empty` |
| 输入区 | `Input.TextArea` |
| 消息提示 | `message` |

## 关键交互流程

### 打开项目

```text
点击 + 打开项目
→ 选择项目文件夹
→ Rust 校验路径是否存在、是否为目录
→ 读取项目名称和 Git 分支
→ 创建或恢复 ProjectSession
→ 新增 Tab
→ 设置为 activeSessionId
→ 加载项目上下文和消息
```

### 切换 Tab

```text
点击项目 Tab
→ 保存当前 UI 临时状态
→ 设置 activeSessionId
→ 加载目标项目的上下文
→ 渲染该项目消息和状态
```

### 关闭 Tab

```text
点击关闭按钮
→ 如果有运行中任务，弹出确认
→ 从 openedSessions 移除
→ 保留 SQLite 中的历史会话
→ 如果关闭的是当前 Tab，切换到相邻 Tab
```

### 应用启动恢复

```text
应用启动
→ 调用 list_open_project_sessions
→ 恢复上次打开的 Tabs
→ 调用 get_active_project_session
→ 激活上次项目
```

## 数据模型

### TypeScript 类型

```typescript
export interface ProjectSession {
  id: string;
  projectName: string;
  projectPath: string;
  status: "idle" | "loading" | "active" | "error";
  gitBranch?: string;
  isOpen: boolean;
  lastActiveAt: string;
  createdAt: string;
}

export interface ProjectSessionContext {
  sessionId: string;
  projectPath: string;
  gitBranch?: string;
  changedFiles: string[];
  pinnedFiles: string[];
  recentFiles: string[];
  currentTask?: string;
  updatedAt: string;
}

export interface ProjectSessionMessage {
  id: string;
  sessionId: string;
  role: "user" | "assistant" | "system";
  content: string;
  createdAt: string;
}
```

### SQLite 表设计

```sql
CREATE TABLE IF NOT EXISTS project_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    project_name TEXT NOT NULL,
    project_path TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'idle',
    git_branch TEXT,
    is_open INTEGER NOT NULL DEFAULT 0,
    last_active_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_session_contexts (
    session_id TEXT PRIMARY KEY NOT NULL,
    project_path TEXT NOT NULL,
    git_branch TEXT,
    changed_files_json TEXT NOT NULL DEFAULT '[]',
    pinned_files_json TEXT NOT NULL DEFAULT '[]',
    recent_files_json TEXT NOT NULL DEFAULT '[]',
    current_task TEXT,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES project_sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS project_session_messages (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES project_sessions(id) ON DELETE CASCADE
);
```

## 后端架构

遵循三层架构：

```text
commands/session.rs
  → services/session_manager.rs
    → database/session.rs
```

### Models 层

建议新增或扩展：

```text
src-tauri/src/models/session.rs
```

包含：

- `ProjectSession`
- `ProjectSessionContext`
- `ProjectSessionMessage`
- `OpenProjectSessionInput`
- `AppendProjectSessionMessageInput`

### Database 层

建议新增：

```text
src-tauri/src/database/session.rs
```

职责：

- 创建或更新项目会话
- 查询打开中的项目会话
- 设置当前打开状态
- 更新最近活跃时间
- 保存和读取上下文
- 保存和读取消息

### Service 层

建议新增：

```text
src-tauri/src/services/session_manager.rs
```

职责：

- 校验项目路径
- 从路径推导项目名称
- 读取 Git 分支和工作区摘要
- 创建或恢复会话
- 处理关闭 Tab 的业务规则
- 组装前端所需 DTO

### Command 层

建议新增：

```text
src-tauri/src/commands/session.rs
```

建议 Command：

```rust
open_project_session(project_path: String) -> Result<ProjectSession, String>
list_open_project_sessions() -> Result<Vec<ProjectSession>, String>
list_recent_project_sessions() -> Result<Vec<ProjectSession>, String>
set_active_project_session(session_id: String) -> Result<(), String>
close_project_session(session_id: String) -> Result<(), String>
get_project_session_context(session_id: String) -> Result<ProjectSessionContext, String>
append_project_session_message(input: AppendProjectSessionMessageInput) -> Result<ProjectSessionMessage, String>
list_project_session_messages(session_id: String) -> Result<Vec<ProjectSessionMessage>, String>
```

## 前端状态设计

建议在现有 Zustand store 中新增 project session slice，或新建独立 store 文件后由 `src/store/index.ts` 导出。

```typescript
interface ProjectSessionStore {
  openedSessions: ProjectSession[];
  activeSessionId: string | null;
  contexts: Record<string, ProjectSessionContext>;
  messages: Record<string, ProjectSessionMessage[]>;
  loading: boolean;

  loadOpenedSessions: () => Promise<void>;
  openProjectSession: (projectPath: string) => Promise<void>;
  closeProjectSession: (sessionId: string) => Promise<void>;
  setActiveSession: (sessionId: string) => Promise<void>;
  loadSessionContext: (sessionId: string) => Promise<void>;
  loadSessionMessages: (sessionId: string) => Promise<void>;
  appendMessage: (sessionId: string, content: string) => Promise<void>;
}
```

## API 封装

在 `src/lib/api/index.ts` 增加：

```typescript
export const projectSessionApi = {
  open: (projectPath: string) => invoke<ProjectSession>("open_project_session", { projectPath }),
  listOpen: () => invoke<ProjectSession[]>("list_open_project_sessions"),
  listRecent: () => invoke<ProjectSession[]>("list_recent_project_sessions"),
  setActive: (sessionId: string) => invoke<void>("set_active_project_session", { sessionId }),
  close: (sessionId: string) => invoke<void>("close_project_session", { sessionId }),
  getContext: (sessionId: string) => invoke<ProjectSessionContext>("get_project_session_context", { sessionId }),
  listMessages: (sessionId: string) => invoke<ProjectSessionMessage[]>("list_project_session_messages", { sessionId }),
  appendMessage: (input: AppendProjectSessionMessageInput) =>
    invoke<ProjectSessionMessage>("append_project_session_message", { input }),
};
```

## 文件夹选择方案

第一版建议使用前端 `@tauri-apps/plugin-dialog` 打开目录选择器，选择结果传给 Rust Command：

```text
前端 dialog.open({ directory: true })
→ 得到 projectPath
→ projectSessionApi.open(projectPath)
```

优点：

- UI 交互自然
- Rust 仍负责最终路径校验和会话创建
- 不让前端直接读取项目文件内容

如果当前项目尚未安装 dialog 插件，需要：

- `src-tauri/Cargo.toml` 增加 `tauri-plugin-dialog`
- `package.json` 增加 `@tauri-apps/plugin-dialog`
- `src-tauri/src/lib.rs` 注册插件
- `src-tauri/capabilities/default.json` 增加 `dialog:default`

## Capabilities

第一版最小权限：

```json
"dialog:default"
```

可选能力：

| 权限 | 使用场景 |
|------|----------|
| `dialog:default` | 选择项目文件夹 |
| `opener:default` | 在系统文件管理器中打开项目目录 |
| `fs:*` | 仅当前端直接读写文件时需要；第一版不建议启用 |
| `shell:*` | 后续启动外部 CLI 或 Git 命令时再评估 |

## 路由和入口

如果已有插件窗口或插件页面，优先把 Tab 化会话窗作为插件任务视图接入；否则新增页面路由：

```text
/pages/project-sessions/index.tsx
```

推荐路由：

```text
/project-sessions
```

后续可以在插件窗口中加载该页面。

## 分阶段实施

### Phase 1：Tab 容器和项目会话骨架

完成：

- 打开项目文件夹
- 创建或恢复 ProjectSession
- 顶部 Tabs 展示多个项目
- 切换 active session
- 关闭 Tab
- 重启恢复已打开 Tabs

不做：

- 真实 Agent 执行
- 终端集成
- Tab 拖拽排序
- 多窗口弹出

### Phase 2：会话消息和上下文

完成：

- 每个项目独立消息列表
- 保存和恢复消息
- 展示 Git 分支、变更文件、最近文件
- 支持 currentTask 和 pinnedFiles

### Phase 3：插件任务绑定项目会话

完成：

- 插件任务绑定 `sessionId`
- 每个项目会话独立运行状态
- 左侧插件工具列表按项目上下文渲染
- Agent 活动流进入主会话区域

### Phase 4：高级体验

可选：

- Tab 拖拽排序
- 项目分组
- 会话归档
- 会话搜索
- 每项目独立模型配置
- 多窗口弹出

## 第一版验收标准

- 可以通过按钮选择项目文件夹
- 选择后新增一个项目 Tab
- 同一路径再次打开时恢复已有会话，不重复创建
- 可以在多个项目 Tab 间切换
- 每个 Tab 展示独立项目路径、Git 分支、状态
- 可以关闭 Tab，关闭后历史会话仍保留
- 重启应用后能恢复上次打开的 Tab 列表
- 代码遵循三层架构和统一 API 封装
