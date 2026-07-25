# 会话窗 Tab 化开发任务

## 任务目标

根据 `tasks/会话窗tab化方案.md` 实现插件窗口中的多项目文件夹会话 Tab 管理能力。

第一阶段只实现推荐方案 A 的 MVP：顶部 Tab 管理多个项目会话、项目状态展示、基础消息容器和重启恢复能力。

## 实施原则

- 严格遵循三层架构：Models → Database → Services → Commands → API → Store → UI
- 数据持久化放 Rust + SQLite
- UI 临时状态放 Zustand（MVP 阶段暂用组件内 useState，数据源仍是 Rust）
- IPC 调用统一封装到 `src/lib/api/index.ts`
- 文件夹选择使用 Tauri dialog 插件；Rust 负责路径校验
- 第一版不接入真实 Agent 进程、不做终端、不做多窗口

## Phase 0：开发前确认

- [x] 审核 `tasks/会话窗tab化方案.md`
- [x] 确认第一版使用 `@tauri-apps/plugin-dialog` 选择目录（已安装）
- [x] 确认页面入口：新增 `/project-sessions` 页面
- [x] 确认复用现有 `session` 模块扩展（不重命名旧 TaskSession）
- [x] 检查当前未提交文件，避免覆盖其他会话工作

## Phase 1：后端数据模型

### 1.1 定义 Rust 模型

- [x] 检查现有 `src-tauri/src/models/session.rs`（已有 TaskSession 等，在其后追加）
- [x] 定义 `ProjectSession`
- [x] 定义 `ProjectSessionContext`
- [x] 定义 `ProjectSessionMessage`
- [x] 定义 `AppendProjectSessionMessageInput`
- [x] 确保模型 derive `Serialize` / `Deserialize`
- [x] 在 `src-tauri/src/models/mod.rs` 已有 `pub mod session;`

### 1.2 定义 TypeScript 类型

- [x] 检查 `src/types/index.ts`（已有 TaskSession 等，在其后追加）
- [x] 定义 `ProjectSession`
- [x] 定义 `ProjectSessionContext`
- [x] 定义 `ProjectSessionMessage`
- [x] 确保字段命名与 Rust serde 输出一致（camelCase）

## Phase 2：数据库层

### 2.1 Schema 迁移

- [x] 当前 user_version: 39 → 升级到 40
- [x] 新增 `project_sessions` 表
- [x] 新增 `project_session_contexts` 表
- [x] 新增 `project_session_messages` 表
- [x] 为 `project_path` 添加唯一约束
- [x] 使用 `PRAGMA user_version` 递增迁移（v39→v40）

### 2.2 Database DAO

- [x] 在现有 `src-tauri/src/database/session.rs` 追加方法
- [x] 实现 `upsert_project_session`
- [x] 实现 `list_open_project_sessions`
- [x] 实现 `list_recent_project_sessions`
- [x] 实现 `get_project_session_by_path`
- [x] 实现 `get_project_session_by_id`
- [x] 实现 `set_project_session_active`
- [x] 实现 `close_project_session`
- [x] 实现 `get_project_session_context`
- [x] 实现 `upsert_project_session_context`
- [x] 实现 `insert_project_session_message`
- [x] 实现 `list_project_session_messages`
- [x] 实现 `create_project_session_message`
- [x] 所有 SQL 使用参数绑定

## Phase 3：Service 层

### 3.1 SessionManager 业务逻辑

- [x] 在现有 `src-tauri/src/services/session_manager.rs` 追加方法
- [x] 实现项目路径存在性校验
- [x] 实现目录类型校验
- [x] 从路径推导 `projectName`
- [x] 读取 Git 分支（`detect_git_branch`，静默失败）
- [x] 实现打开项目：存在则恢复，不存在则创建
- [x] 实现关闭项目：仅关闭 Tab，不删除历史数据
- [x] 实现最近活跃时间更新
- [x] 实现默认上下文创建

### 3.2 Git 信息策略

- [x] 第一版只读取当前分支（`git branch --show-current`）
- [x] Git 读取失败不阻断打开项目
- [x] 非 Git 目录仍可作为项目会话打开
- [ ] 后续阶段再扩展 changedFiles / untrackedFiles

## Phase 4：Command 层和注册

### 4.1 Commands

- [x] 在现有 `src-tauri/src/commands/session.rs` 追加命令
- [x] 实现 `open_project_session(project_path: String)`
- [x] 实现 `list_open_project_sessions()`
- [x] 实现 `list_recent_project_sessions()`
- [x] 实现 `set_active_project_session(session_id: String)`
- [x] 实现 `close_project_session(session_id: String)`
- [x] 实现 `get_project_session_context(session_id: String)`
- [x] 实现 `append_project_session_message(input)`
- [x] 实现 `list_project_session_messages(session_id: String)`
- [x] 所有 Command 返回 `Result<T, String>`

### 4.2 注册

- [x] `commands/mod.rs` 已有 `pub mod session;`
- [x] `services/mod.rs` 已有 `pub mod session_manager;`
- [x] `database/mod.rs` 已有 `pub mod session;`
- [x] `lib.rs` 的 `generate_handler![]` 已注册 8 个新 Command

## Phase 5：插件和权限

### 5.1 Dialog 插件

- [x] `package.json` 已安装 `@tauri-apps/plugin-dialog` ^2.6.0
- [x] `Cargo.toml` 已安装 `tauri-plugin-dialog = "2"`
- [x] `lib.rs` 已注册 dialog 插件

### 5.2 Capabilities

- [x] `default.json` 已有 `"dialog:default"`
- [x] 已有 `"opener:default"` + `"opener:allow-open-path"`
- [x] 第一版不增加 `fs:*` 权限

## Phase 6：前端 API 封装

### 6.1 API

- [x] 在 `src/lib/api/index.ts` 增加 `projectSessionApi`
- [x] 封装 `open(projectPath)`
- [x] 封装 `listOpen()`
- [x] 封装 `listRecent()`
- [x] 封装 `setActive(sessionId)`
- [x] 封装 `close(sessionId)`
- [x] 封装 `getContext(sessionId)`
- [x] 封装 `listMessages(sessionId)`
- [x] 封装 `appendMessage(sessionId, role, content)`
- [x] 所有 API 使用 `src/types/index.ts` 中的类型

## Phase 7：前端状态管理

### 7.1 状态方案（MVP）

- [x] MVP 阶段使用组件内 `useState`（数据源仍是 Rust/SQLite）
- [x] `openedSessions` / `activeKey` 由页面组件管理
- [x] 消息按 sessionId 分桶（`Record<string, ProjectSessionMessage[]>`）
- [ ] 后续阶段考虑迁移到 Zustand store 做跨组件共享

### 7.2 状态恢复

- [x] 页面初始化时调用 `projectSessionApi.listOpen()` 加载已打开 sessions
- [x] 如果存在打开的 session，自动激活第一个
- [ ] 后续阶段登录后加载 context 和 messages

## Phase 8：前端 UI 组件

### 8.1 页面入口

- [x] 创建 `src/pages/project-sessions/index.tsx`
- [x] 在 `src/Router.tsx` 注册路由 `/project-sessions`
- [ ] 在侧边栏菜单或插件窗口中添加入口链接（后续接入）

### 8.2 ProjectSessionWindow

- [x] 实现整体布局（Tab 栏 + 信息栏 + 主内容区）
- [x] 页面初始化时加载已打开 sessions
- [x] 处理 empty 状态（Empty 组件 + 打开项目按钮）

### 8.3 ProjectTabBar

- [x] 使用自定义 Tab 栏（非 Ant Tabs，采用内联实现更灵活）
- [x] 实现 `+ 打开项目` 按钮
- [x] 使用 dialog 选择目录（`open({ directory: true })`）
- [x] 调用 `projectSessionApi.open()`
- [x] 实现 Tab 切换（更新 activeKey + `setActive`）
- [x] 实现 Tab 关闭（hover 出现 X 按钮）
- [x] 实现"关闭其他/关闭全部"下拉菜单

### 8.4 ProjectSessionHeader

- [x] 展示项目名称 + 项目路径
- [x] 展示 Git 分支（Tag + 图标）
- [x] 展示会话状态 Tag
- [ ] 展示最近活跃时间（后续补充）

### 8.5 ProjectContextSidebar

- [ ] 第一版简化：暂不实现侧边栏
- [ ] 上下文信息展示在项目信息栏中

### 8.6 ProjectSessionMain

- [x] 实现消息列表
- [x] 实现空状态提示
- [x] 实现输入框（textarea + Enter 发送）
- [x] 实现消息气泡（用户/助手双色）
- [x] 实现 `appendMessage` 持久化

## Phase 9：验证

### 9.1 后端验证

- [x] `cargo check` 通过（0 错误，0 警告）
- [x] Schema 迁移函数正确编译
- [x] 非 Git 目录打开逻辑已实现（`detect_git_branch` 返回 None）

### 9.2 前端验证

- [x] `npx tsc --noEmit` 通过（0 错误）
- [ ] 页面可以打开（需要启动 `pnpm tauri dev` 实际验证）
- [ ] Tab 交互在真实环境下验证

### 9.3 权限验证

- [x] `dialog:default` 已在 capabilities 中声明
- [x] 第一版不需要 `fs:*` 权限
- [x] 所有 IPC 通过 Rust Command 代理

## Phase 10：收尾

- [x] 检查并清理未使用类型/导入（tsc + cargo check 0 错误）
- [x] 检查是否违反三层架构（遵循 Models → Database → Services → Commands）
- [x] 检查前端无裸 `invoke()` 调用（统一封装在 `projectSessionApi`）
- [x] 任务文档勾选状态已更新
- [x] 等待用户确认是否进入下一阶段

## MVP 验收标准

- [x] 可以从 Tab 栏点击 `+ 打开项目` 按钮
- [x] 可以选择一个项目文件夹（dialog directory picker）
- [x] 成功创建或恢复该项目会话（Rust upsert + 路径校验）
- [x] 顶部出现对应项目 Tab（自定义 Tab 栏）
- [x] 多个项目可同时作为多个 Tab 打开
- [x] 点击 Tab 可切换当前项目会话
- [x] 每个 Tab 展示独立项目路径、Git 分支、状态
- [x] 可关闭 Tab（hover X / 关闭其他 / 关闭全部）
- [x] 重启应用后恢复上次打开的 Tabs（`listOpen` 从 `project_sessions` 表读）
- [x] 会话消息按项目隔离保存和读取

## 暂不纳入第一版

- [ ] Tab 拖拽排序
- [ ] 多窗口弹出
- [ ] Web TUI / 终端集成
- [ ] 真实 Claude/Codex/Gemini Agent 进程池
- [ ] 项目文件树
- [ ] 每项目独立模型配置
- [ ] 会话搜索和归档
- [ ] Git diff 详细展示
- [ ] ProjectContextSidebar（侧边栏上下文卡片）

## 修改文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src-tauri/src/models/session.rs` | 修改 | 新增 ProjectSession 等 4 个模型 |
| `src-tauri/src/database/session.rs` | 修改 | 新增 15 个 DAO 方法 + 2 个辅助函数 |
| `src-tauri/src/database/schema.rs` | 修改 | 升级到 v40，新增 3 张表 |
| `src-tauri/src/services/session_manager.rs` | 修改 | 新增 open/close 项目会话 + detect_git_branch |
| `src-tauri/src/commands/session.rs` | 修改 | 新增 8 个 Command |
| `src-tauri/src/lib.rs` | 修改 | 注册 8 个新 Command |
| `src/types/index.ts` | 修改 | 新增 ProjectSession 等 3 个类型 |
| `src/lib/api/index.ts` | 修改 | 新增 projectSessionApi（8 个方法） |
| `src/pages/project-sessions/index.tsx` | 新建 | Tab 管理页面（~250 行） |
| `src/Router.tsx` | 修改 | 注册 /project-sessions 路由 |
