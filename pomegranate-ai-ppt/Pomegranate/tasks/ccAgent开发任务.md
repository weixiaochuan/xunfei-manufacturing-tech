# Claude Code Agent 集成开发任务

## 背景

采用 **A + D 组合方案**：

- **方案 A：应用内 Claude Code Agent Runner**  
  Rust 后端直接启动本机 `claude` CLI 子进程，应用内展示流式输出、管理会话生命周期、保存执行日志。

- **方案 D：现有 MCP 反向集成增强**  
  继续强化当前 kb-core MCP server 能力，让外部 Claude Code CLI 能通过 MCP 调用本应用知识库工具。

## 总体目标

在当前 Tauri 三层架构下新增 `Claude Code Agent` 子系统，使用户可以在项目会话/任务会话中：

1. 检测本机 Claude Code CLI 可用性。
2. 将本应用 MCP server 一键安装到 Claude Code 配置。
3. 在应用内选择项目目录、输入任务 prompt、启动 Claude Code agent。
4. 实时查看 stdout/stderr/状态事件。
5. 停止正在运行的 agent。
6. 持久化 agent 会话和事件日志。
7. 将 agent 执行能力与 ProjectSession / TaskSession 逐步融合。

---

## 架构决策

### 已采纳方案

采用 **A + D**：

| 方向 | 方案 | 说明 |
|------|------|------|
| 应用内执行 | Rust `tokio::process::Command` 启动 `claude` CLI | 由后端管理生命周期、流式输出和审计日志 |
| 外部联动 | Claude Code 通过 MCP 连接本应用 | 复用现有 `commands::mcp` / kb-core MCP 工具 |

### 不采用方案

| 方案 | 暂不采用原因 |
|------|--------------|
| PTY 终端完整模拟 | MVP 复杂度过高，跨平台输入/ANSI/交互处理成本大 |
| Node sidecar + Claude Agent SDK | 打包和运行时复杂度高，先验证 CLI 子进程路径 |
| 复用现有 AiProvider | Claude Code 是执行型 agent，不是普通 chat model provider，生命周期和安全边界不同 |

---

## Phase 0：现状梳理与接口确认

### 0.1 阅读现有 MCP 与会话实现

- [ ] 阅读 `src-tauri/src/commands/mcp.rs`
  - [ ] 确认 `mcp_get_claude_md_template`
  - [ ] 确认 `mcp_install_to_client`
  - [ ] 确认 `InstallTarget::ClaudeCode`
- [ ] 阅读 `src-tauri/src/lib.rs` 中 MCP 初始化逻辑
  - [ ] 确认 in-memory MCP server 启动方式
  - [ ] 确认 `AppState.mcp_internal` 现有字段
- [ ] 阅读项目会话实现
  - [ ] `src-tauri/src/commands/session.rs`
  - [ ] `src-tauri/src/services/session_manager.rs`
  - [ ] `src-tauri/src/database/session.rs`
  - [ ] `src/pages/project-sessions/index.tsx`
  - [ ] `src/pages/task-session/index.tsx`

### 0.2 确认 Claude Code CLI 调用形态

- [ ] 在开发机验证 `claude --version` / `claude --help`
- [ ] 明确非交互启动参数
  - [ ] 是否支持一次性 prompt
  - [ ] 是否支持指定工作目录
  - [ ] 是否支持输出格式参数
  - [ ] 是否支持 permission mode 参数
- [ ] 明确 MVP 使用方式
  - [ ] 优先使用非 PTY、非交互模式
  - [ ] 暂不处理复杂交互式确认

---

## Phase 1：强化 Claude Code MCP 集成（方案 D）

### 1.1 后端：完善 Claude Code MCP 检测

- [x] 新增/复用 CLI 检测 Command
  - [x] 检测 `claude` 是否在 PATH 中
  - [x] 获取版本号
  - [x] 检查 `~/.claude.json` 或 `CLAUDE_CONFIG_DIR/.claude.json` 是否存在
- [x] 返回结构体：

```rust
pub struct ClaudeCodeCliInfo {
    pub available: bool,
    pub version: Option<String>,
    pub config_path: Option<String>,
    pub mcp_installed: bool,
    pub error: Option<String>,
}
```

- [x] 注意 Windows 子进程使用 `CREATE_NO_WINDOW`
- [x] Windows 兼容 npm shim：额外检测 `%APPDATA%\\npm\\claude.cmd` / `claude.exe`

### 1.2 后端：增强现有 MCP 安装状态

- [x] 复查 `mcp_check_install_status` 是否覆盖 Claude Code
- [x] 如果缺失，补齐 Claude Code 配置检测
- [x] 区分只读 / writable 两种配置状态
- [x] 确保写配置前做 JSON 解析和备份策略

### 1.3 前端：设置页 Claude Code 集成卡片

- [x] 在现有 MCP 设置 UI 中增加/完善 Claude Code CLI 卡片
- [x] 展示：
  - [x] CLI 是否安装
  - [x] CLI 版本
  - [x] 配置文件路径
  - [x] MCP 安装状态
  - [x] sidecar 路径
  - [x] DB 路径
- [ ] 操作按钮：
  - [x] 一键安装只读 MCP
  - [x] 一键安装 writable MCP
  - [ ] 卸载 MCP
  - [x] 复制 `CLAUDE.md` 模板
  - [x] 复制 settings snippet

### 1.4 验证

- [ ] Claude Code CLI 中能看到 `knowledge-base` MCP server
- [ ] Claude Code CLI 能调用只读工具
- [ ] writable 模式下能创建/更新笔记
- [x] 安装/卸载不会破坏用户已有配置

---

## Phase 2：Agent Runner 后端基础（方案 A）

### 2.1 数据模型

新增模型文件或扩展 `models/mod.rs`：

- [ ] `ClaudeAgentSession`
- [ ] `ClaudeAgentEvent`
- [ ] `StartClaudeAgentInput`
- [ ] `ClaudeAgentCliInfo`
- [ ] `ClaudeAgentStatus`
- [ ] `ClaudeAgentPermissionMode`

建议 TypeScript 对齐：

```ts
export type ClaudeAgentStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export type ClaudeAgentPermissionMode =
  | "readonly"
  | "ask"
  | "workspace_write";
```

### 2.2 数据库 Schema

- [ ] 新增迁移版本
- [ ] 创建 `claude_agent_sessions` 表

```sql
CREATE TABLE IF NOT EXISTS claude_agent_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    project_path TEXT NOT NULL,
    prompt TEXT NOT NULL,
    session_name TEXT,
    linked_task_session_id TEXT,
    linked_project_session_id TEXT,
    permission_mode TEXT NOT NULL,
    status TEXT NOT NULL,
    pid INTEGER,
    exit_code INTEGER,
    error_message TEXT,
    created_at DATETIME DEFAULT (datetime('now', 'localtime')),
    updated_at DATETIME DEFAULT (datetime('now', 'localtime')),
    started_at DATETIME,
    finished_at DATETIME
);
```

- [ ] 创建 `claude_agent_events` 表

```sql
CREATE TABLE IF NOT EXISTS claude_agent_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at DATETIME DEFAULT (datetime('now', 'localtime')),
    FOREIGN KEY(session_id) REFERENCES claude_agent_sessions(id) ON DELETE CASCADE
);
```

- [ ] 添加索引

```sql
CREATE INDEX IF NOT EXISTS idx_claude_agent_sessions_project_path
ON claude_agent_sessions(project_path);

CREATE INDEX IF NOT EXISTS idx_claude_agent_events_session_id
ON claude_agent_events(session_id);
```

### 2.3 Database 层

新增 `src-tauri/src/database/claude_agent.rs` 或在现有 database 模块中拆分：

- [ ] `create_claude_agent_session(input) -> ClaudeAgentSession`
- [ ] `update_claude_agent_session_status(id, status, exit_code, error)`
- [ ] `set_claude_agent_pid(id, pid)`
- [ ] `list_claude_agent_sessions(project_path)`
- [ ] `get_claude_agent_session(id)`
- [ ] `add_claude_agent_event(session_id, kind, content)`
- [ ] `list_claude_agent_events(session_id)`

要求：

- [ ] SQL 全部使用参数绑定
- [ ] Mutex 加锁使用 `map_err`
- [ ] 不使用 `unwrap()` 处理可失败路径

### 2.4 AppState 运行时状态

在 `src-tauri/src/state.rs` 增加运行中进程表：

```rust
pub claude_agents: Arc<Mutex<HashMap<String, ClaudeAgentProcessHandle>>>;
```

`ClaudeAgentProcessHandle` 建议包含：

- [ ] session id
- [ ] pid
- [ ] cancel channel / kill handle
- [ ] started_at

注意：

- [ ] 不把 `tokio::process::Child` 直接长期裸暴露给 Command
- [ ] 设计停止机制，避免锁跨 await

---

## Phase 3：Agent Runner Service 与 Command

### 3.1 Service 层：CLI 检测

新增 `src-tauri/src/services/claude_agent.rs`：

- [ ] `check_cli() -> ClaudeAgentCliInfo`
- [ ] 跨平台查找 `claude`
- [ ] 执行 `claude --version`
- [ ] 解析版本输出
- [ ] Windows 设置 `CREATE_NO_WINDOW`

### 3.2 Service 层：输入校验和安全边界

- [ ] 校验 `project_path` 必须存在且是目录
- [ ] 禁止危险目录作为工作目录
  - [ ] 磁盘根目录
  - [ ] 用户 HOME 根目录
  - [ ] 应用数据目录
  - [ ] 系统目录
- [ ] 校验 prompt 非空
- [ ] 权限模式默认 `ask` 或 `readonly`
- [ ] 高权限模式要求前端显式确认后传入 `confirmed=true`

### 3.3 Service 层：启动 Claude Code

- [ ] `start_session(app, db, state, input) -> ClaudeAgentSession`
- [ ] 创建 DB session，初始状态 `pending`
- [ ] 构造 `tokio::process::Command`
  - [ ] program: `claude`
  - [ ] current_dir: `project_path`
  - [ ] stdin/stdout/stderr 管道配置
  - [ ] Windows 设置 `CREATE_NO_WINDOW`
- [ ] 启动后记录 pid
- [ ] 状态改为 `running`
- [ ] stdout reader task
- [ ] stderr reader task
- [ ] wait task 监听进程退出
- [ ] 退出后更新状态为 `completed` / `failed` / `cancelled`

### 3.4 Service 层：事件推送

统一事件名：

```text
claude-agent:started
claude-agent:chunk
claude-agent:stderr
claude-agent:status
claude-agent:error
claude-agent:done
```

Payload：

```ts
export interface ClaudeAgentEventPayload {
  sessionId: string;
  kind: "stdout" | "stderr" | "status" | "error" | "done";
  content: string;
  createdAt: string;
}
```

要求：

- [ ] 每条 stdout/stderr 同时写入 DB 和 emit 前端
- [ ] emit 失败不应导致 agent 崩溃
- [ ] DB 写入失败需要 emit error 并记录日志

### 3.5 Service 层：停止会话

- [ ] `stop_session(state, db, session_id)`
- [ ] 查找运行中进程
- [ ] 触发 kill / cancel
- [ ] 状态更新为 `cancelled`
- [ ] 发送 `claude-agent:done`
- [ ] 清理运行时进程表

### 3.6 Command 层

新增 `src-tauri/src/commands/claude_agent.rs`：

- [ ] `claude_agent_check_cli`
- [ ] `start_claude_agent_session`
- [ ] `stop_claude_agent_session`
- [ ] `list_claude_agent_sessions`
- [ ] `list_claude_agent_events`
- [ ] `get_claude_agent_session`

注册：

- [ ] `commands/mod.rs` 导出 `claude_agent`
- [ ] `lib.rs generate_handler![]` 注册所有 Command

---

## Phase 4：前端 API、状态与 UI

### 4.1 TypeScript 类型

在 `src/types/index.ts` 增加：

- [ ] `ClaudeAgentCliInfo`
- [ ] `ClaudeAgentSession`
- [ ] `ClaudeAgentEvent`
- [ ] `ClaudeAgentEventPayload`
- [ ] `StartClaudeAgentInput`
- [ ] `ClaudeAgentStatus`
- [ ] `ClaudeAgentPermissionMode`

### 4.2 API 封装

在 `src/lib/api/index.ts` 增加：

```ts
export const claudeAgentApi = {
  checkCli: () => invoke<ClaudeAgentCliInfo>("claude_agent_check_cli"),
  start: (input: StartClaudeAgentInput) =>
    invoke<ClaudeAgentSession>("start_claude_agent_session", { input }),
  stop: (sessionId: string) =>
    invoke<void>("stop_claude_agent_session", { sessionId }),
  list: (projectPath?: string) =>
    invoke<ClaudeAgentSession[]>("list_claude_agent_sessions", { projectPath }),
  get: (sessionId: string) =>
    invoke<ClaudeAgentSession>("get_claude_agent_session", { sessionId }),
  events: (sessionId: string) =>
    invoke<ClaudeAgentEvent[]>("list_claude_agent_events", { sessionId }),
};
```

### 4.3 Zustand 状态

按需扩展 `src/store/index.ts` 或新建局部 store：

- [ ] 当前 active agent session
- [ ] running session ids
- [ ] session output buffer
- [ ] agent 面板打开状态
- [ ] CLI 检测结果缓存

### 4.4 UI 组件

新增：

```text
src/components/ai/ClaudeAgentPanel.tsx
src/components/ai/ClaudeAgentConsole.tsx
src/components/ai/ClaudeAgentControlBar.tsx
src/components/ai/ClaudeAgentSessionList.tsx
src/components/ai/ClaudeAgentStartModal.tsx
```

组件职责：

- [ ] `ClaudeAgentPanel`：整体容器
- [ ] `ClaudeAgentStartModal`：启动表单
- [ ] `ClaudeAgentConsole`：stdout/stderr 输出
- [ ] `ClaudeAgentControlBar`：启动、停止、清空、复制输出
- [ ] `ClaudeAgentSessionList`：历史会话列表

Ant Design 组件：

- [ ] `Form`
- [ ] `Input.TextArea`
- [ ] `Select`
- [ ] `Button`
- [ ] `Modal`
- [ ] `Tag`
- [ ] `Alert`
- [ ] `Timeline` 或 `List`
- [ ] `Spin`

### 4.5 事件监听

- [ ] 在 Agent 面板挂载时监听：
  - [ ] `claude-agent:started`
  - [ ] `claude-agent:chunk`
  - [ ] `claude-agent:stderr`
  - [ ] `claude-agent:status`
  - [ ] `claude-agent:error`
  - [ ] `claude-agent:done`
- [ ] unmount 时清理所有 `unlisten`
- [ ] 按 `sessionId` 过滤事件，避免多会话串流互相污染

---

## Phase 5：与 ProjectSession / TaskSession 融合

### 5.1 ProjectSession 集成

- [ ] 在项目会话页面增加 “Claude Code Agent” Tab / 面板
- [ ] 自动使用当前 `project_path`
- [ ] 展示该项目的 agent 历史
- [ ] 支持从项目上下文生成 prompt

### 5.2 TaskSession 集成

- [ ] 在任务阶段控制区增加 “交给 Claude Code” 操作
- [ ] 自动拼接 prompt：

```text
请在当前项目中完成以下阶段任务：

阶段名称：...
阶段目标：...
验收标准：...
注意：遵循项目 CLAUDE.md，不要覆盖用户未提交改动。
```

- [ ] 记录 `linked_task_session_id`
- [ ] Agent 完成后写入执行日志
- [ ] 用户确认后再推进 phase

### 5.3 执行结果回写

- [ ] Agent 会话结束后生成简短执行摘要
- [ ] 写入 `execution_logs`
- [ ] 在 TaskSession UI 中展示关联 agent session

---

## Phase 6：安全与权限控制

### 6.1 启动前确认

- [ ] `readonly` 模式可轻确认
- [ ] `ask` / `workspace_write` 模式必须弹出高风险确认
- [ ] Modal 展示：
  - [ ] 工作目录
  - [ ] 权限模式
  - [ ] 可能修改文件/执行命令
  - [ ] 是否启用 MCP writable

### 6.2 工作目录安全

- [ ] 禁止根目录
- [ ] 禁止应用数据目录
- [ ] 禁止系统目录
- [ ] 禁止空路径
- [ ] 路径 canonicalize 后再比较

### 6.3 多会话并发保护

- [ ] 同一 project_path 默认只允许一个 running agent
- [ ] 如需并发，提示使用 worktree
- [ ] 不自动执行 `git stash` / `git reset` / `git clean`
- [ ] 不自动 kill 非本系统启动的进程

### 6.4 审计

- [ ] 所有启动参数入库
- [ ] 所有 stdout/stderr 入库
- [ ] 退出码入库
- [ ] 错误信息入库
- [ ] UI 提供复制日志功能

---

## Phase 7：测试与验证

### 7.1 Rust 测试

- [ ] Database CRUD 测试
- [ ] 工作目录安全校验测试
- [ ] CLI 检测输出解析测试
- [ ] 状态流转测试

### 7.2 前端测试/手测

- [ ] CLI 未安装提示正确
- [ ] MCP 未安装提示正确
- [ ] 启动表单校验正确
- [ ] stdout/stderr 实时显示
- [ ] 停止按钮可取消运行
- [ ] 历史日志可恢复

### 7.3 跨平台验证

- [ ] Windows：无 CMD 弹窗
- [ ] Windows：路径含空格可运行
- [ ] macOS：能找到 `claude`
- [ ] Linux：能找到 `claude`

---

## MVP 验收标准

- [ ] 设置页能检测 Claude Code CLI。
- [ ] 设置页能安装/卸载 Claude Code MCP 配置。
- [ ] 应用内能启动一次 Claude Code agent。
- [ ] 能看到实时 stdout/stderr。
- [ ] 能停止正在运行的 agent。
- [ ] 会话和事件日志持久化到 SQLite。
- [ ] 重启应用后能查看历史 agent 会话和日志。
- [ ] 同一项目默认防止重复启动多个 agent。
- [ ] Windows 打包后启动子进程不弹 CMD 黑窗口。

---

## 后续增强方向

- [ ] 支持 PTY 交互模式
- [ ] 支持 Claude Code 原生权限确认 UI 映射
- [ ] 支持 diff 摘要展示
- [ ] 支持自动读取 git diff 生成执行报告
- [ ] 支持 worktree 一键创建隔离执行环境
- [ ] 支持多 agent 并发队列
- [ ] 支持从笔记/任务自动生成 prompt 模板
- [ ] 支持 agent 执行完成后系统通知
