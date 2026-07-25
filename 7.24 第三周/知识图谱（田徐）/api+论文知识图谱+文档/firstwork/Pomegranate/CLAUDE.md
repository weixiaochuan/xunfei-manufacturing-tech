# CLAUDE.md - Tauri Desktop App Framework

## 语言设置

**必须使用中文**与用户对话。

## 术语约定

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>术语</p></th><th colspan="1" rowspan="1"><p>含义</p></th><th colspan="1" rowspan="1"><p>对应目录</p></th></tr><tr><td colspan="1" rowspan="1"><p><strong>后端</strong></p></td><td colspan="1" rowspan="1"><p>Rust Core（Tauri 后端进程）</p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>前端</strong></p></td><td colspan="1" rowspan="1"><p>React UI（WebView 进程）</p></td><td colspan="1" rowspan="1"><p><code>src/</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>配置</strong></p></td><td colspan="1" rowspan="1"><p>Tauri 核心配置</p></td><td colspan="1" rowspan="1"><p><code>src-tauri/tauri.conf.json</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>权限</strong></p></td><td colspan="1" rowspan="1"><p>Capabilities 安全声明</p></td><td colspan="1" rowspan="1"><p><code>src-tauri/capabilities/</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Command</strong></p></td><td colspan="1" rowspan="1"><p>Rust 侧可被前端调用的函数</p></td><td colspan="1" rowspan="1"><p><code>#[tauri::command]</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>IPC</strong></p></td><td colspan="1" rowspan="1"><p>进程间通信（前端 ↔ Rust）</p></td><td colspan="1" rowspan="1"><p><code>invoke()</code> / <code>listen()</code></p></td></tr></tbody></table>

* * *

## 核心架构（必须牢记）

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>项目</p></th><th colspan="1" rowspan="1"><p>规范</p></th></tr><tr><td colspan="1" rowspan="1"><p><strong>应用类型</strong></p></td><td colspan="1" rowspan="1"><p>Tauri 2.x 桌面应用（双进程架构）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>后端语言</strong></p></td><td colspan="1" rowspan="1"><p>Rust 2021 edition</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>前端框架</strong></p></td><td colspan="1" rowspan="1"><p>React 19 + TypeScript 5.8</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>UI 组件库</strong></p></td><td colspan="1" rowspan="1"><p>Ant Design (v5+) + Lucide React 图标库</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>样式方案</strong></p></td><td colspan="1" rowspan="1"><p>TailwindCSS 4 + CSS Variables 设计令牌</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>状态管理</strong></p></td><td colspan="1" rowspan="1"><p>Zustand (v5+)（全局状态）+ React Hooks（局部状态）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>路由方案</strong></p></td><td colspan="1" rowspan="1"><p>React Router 7（HashRouter）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>构建工具</strong></p></td><td colspan="1" rowspan="1"><p>Vite 7 (前端) + Cargo (后端)</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>通信机制</strong></p></td><td colspan="1" rowspan="1"><p>Tauri IPC（<code>invoke</code> 调用 Rust Commands）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>序列化</strong></p></td><td colspan="1" rowspan="1"><p>serde + serde_json（Rust ↔ JSON ↔ TypeScript）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>数据库</strong></p></td><td colspan="1" rowspan="1"><p>SQLite（rusqlite，Rust 直接操作）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>错误处理</strong></p></td><td colspan="1" rowspan="1"><p>thiserror（Rust）+ ErrorBoundary（React）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>安全模型</strong></p></td><td colspan="1" rowspan="1"><p>Capabilities 细粒度权限声明</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>应用标识</strong></p></td><td colspan="1" rowspan="1"><p><code>edu.bit.inb</code></p></td></tr></tbody></table>

### 双进程架构

```
┌───────────────────────────────────────────────────────┐
│                     Tauri 应用                         │
│                                                       │
│  ┌──────────────────┐  IPC (invoke)  ┌──────────────────┐
│  │   WebView 进程    │ ◄════════════► │   Rust Core 进程  │
│  │                  │                │                  │
│  │  React 19        │  Commands      │  commands/       │
│  │  Ant Design 5    │  Events        │  services/       │
│  │  TailwindCSS 4   │  ────────►     │  database/       │
│  │  Zustand         │                │  models/         │
│  │  React Router    │  ◄────────     │  error.rs        │
│  │                  │  返回值         │  state.rs        │
│  │  UI 渲染         │                │                  │
│  │  用户交互        │                │  系统 API        │
│  │  前端状态        │                │  文件操作        │
│  │                  │                │  SQLite 数据库   │
│  └──────────────────┘                └──────────────────┘
└───────────────────────────────────────────────────────┘
```

### 后端三层架构

```
Commands 层（IPC 入口）→ Services 层（业务逻辑）→ Database 层（数据访问）
```

### 分层职责

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>层级</p></th><th colspan="1" rowspan="1"><p>职责</p></th><th colspan="1" rowspan="1"><p>关键技术</p></th></tr><tr><td colspan="1" rowspan="1"><p><strong>WebView 层</strong></p></td><td colspan="1" rowspan="1"><p>UI 渲染、用户交互</p></td><td colspan="1" rowspan="1"><p>React 19 + Ant Design + TailwindCSS</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>状态管理层</strong></p></td><td colspan="1" rowspan="1"><p>全局状态、设置管理</p></td><td colspan="1" rowspan="1"><p>Zustand（<code>src/store/</code>）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>API 封装层</strong></p></td><td colspan="1" rowspan="1"><p>统一 invoke 调用</p></td><td colspan="1" rowspan="1"><p><code>src/lib/api/index.ts</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>IPC 桥接层</strong></p></td><td colspan="1" rowspan="1"><p>前后端通信</p></td><td colspan="1" rowspan="1"><p><code>invoke()</code> / <code>listen()</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Command 层</strong></p></td><td colspan="1" rowspan="1"><p>IPC 接口定义</p></td><td colspan="1" rowspan="1"><p><code>#[tauri::command]</code>（<code>src-tauri/src/commands/</code>）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Service 层</strong></p></td><td colspan="1" rowspan="1"><p>业务逻辑</p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/services/</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Database 层</strong></p></td><td colspan="1" rowspan="1"><p>数据访问 DAO</p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/database/</code>（rusqlite）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Plugin 层</strong></p></td><td colspan="1" rowspan="1"><p>功能扩展</p></td><td colspan="1" rowspan="1"><p><code>tauri::Builder.plugin()</code> 注册</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Capabilities 层</strong></p></td><td colspan="1" rowspan="1"><p>安全权限控制</p></td><td colspan="1" rowspan="1"><p>JSON 声明式权限</p></td></tr></tbody></table>

* * *

## 目录结构

```
tauri/
├── index.html                    # HTML 入口（SPA 挂载点）
├── package.json                  # Node.js 依赖和脚本
├── tsconfig.json                 # TypeScript 配置（含 @/ 路径别名）
├── vite.config.ts                # Vite 构建配置（TailwindCSS + 路径别名）
│
├── src/                          # ★ 前端源码（React + TypeScript）
│   ├── main.tsx                  # 前端入口（ReactDOM.createRoot）
│   ├── App.tsx                   # 主组件（ConfigProvider + 主题 + ErrorBoundary）
│   ├── Router.tsx                # 路由配置（React Router）
│   ├── vite-env.d.ts             # Vite 类型声明
│   ├── theme/                    # 主题配置
│   │   └── antdTheme.ts          # Ant Design 暗色/亮色主题
│   ├── styles/                   # 样式系统
│   │   ├── variables.css         # CSS 设计令牌（颜色/间距/圆角）
│   │   └── global.css            # TailwindCSS + 全局样式
│   ├── store/
│   │   └── index.ts              # Zustand 全局状态（主题/侧边栏）
│   ├── types/
│   │   └── index.ts              # TypeScript 类型定义（与 Rust 对齐）
│   ├── hooks/
│   │   └── useCommand.ts         # useCommand Hook + safeInvoke 工具
│   ├── lib/
│   │   └── api/
│   │       └── index.ts          # API 调用封装（systemApi / configApi）
│   ├── components/
│   │   ├── ui/
│   │   │   └── ErrorBoundary.tsx  # 错误边界组件
│   │   └── layout/
│   │       ├── AppLayout.tsx      # 应用布局（Sider + Header + Content）
│   │       └── Sidebar.tsx        # 侧边栏导航
│   └── pages/
│       ├── home/
│       │   └── index.tsx          # 首页
│       ├── settings/
│       │   └── index.tsx          # 设置页（配置管理）
│       └── about/
│           └── index.tsx          # 关于页（系统信息）
│
├── src-tauri/                    # ★ Rust 后端（Tauri Core）
│   ├── Cargo.toml                # Rust 依赖配置
│   ├── build.rs                  # Tauri 构建脚本
│   ├── tauri.conf.json           # ★ Tauri 核心配置
│   ├── capabilities/             # ★ 权限声明
│   │   └── default.json
│   ├── icons/                    # 应用图标
│   └── src/
│       ├── main.rs               # Rust 进程入口
│       ├── lib.rs                # ★ 核心入口（Builder + 插件 + Command 注册）
│       ├── error.rs              # ★ 统一错误类型（AppError + thiserror）
│       ├── state.rs              # ★ 应用状态（AppState + Database）
│       ├── models/
│       │   └── mod.rs            # 数据模型（AppConfig / SystemInfo）
│       ├── database/
│       │   ├── mod.rs            # 数据库操作（Database struct + DAO）
│       │   └── schema.rs         # 表结构迁移（PRAGMA user_version）
│       ├── services/
│       │   ├── mod.rs            # 服务层入口
│       │   └── config.rs         # 配置业务逻辑
│       └── commands/
│           ├── mod.rs            # Command 模块入口
│           ├── system.rs         # 系统 Commands（greet / get_system_info）
│           └── config.rs         # 配置 Commands（CRUD）
│
├── public/                       # 静态资源
└── docs/                         # 项目文档
```

* * *

## 🔴 Skills 强制评估（必须遵守）

> **每次用户提问时，Hook 会注入技能评估提示。必须严格遵循！**

**流程**：

1.  **评估**：根据注入的技能列表，列出匹配的技能及理由
    
2.  **激活**：对每个匹配的技能调用 `Skill(技能名)`
    
3.  **实现**：激活完成后开始实现
    

* * *

## 🔴 多会话并发自动避让协议（L1/L2/L3 三层触发）

> 用户可能同时开多个 Claude Code 会话操作同一仓库。本会话必须**自动感知并避让**其他会话的工作，**默认静默执行，不打扰用户**。 设计原则：宁可绕路，绝不覆盖；宁可静默放弃，绝不擅自 stash / reset / checkout。

### L1 — 启动时探测（首次响应前，仅执行一次）

```bash
git status -s
git branch --show-current
```

-   把"未提交文件清单"和"当前分支"记入会话上下文，整个会话复用，**不向用户复述**
    
-   若清单非空且与本次任务无关 → 视为"他者占用区"，本会话**不修改、不 stash、不 checkout、不 reset** 这些文件
    
-   若清单非空且与本次任务相关（用户接续之前的工作）→ 当作己方未完成工作正常处理
    

### L2 — 修改文件前（按需触发，单文件粒度）

修改任意已存在文件**之前**，执行：

```bash
git log -1 --format="%ar|%s" <file>
```

判定规则（严格按此执行，不询问）：

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>条件</p></th><th colspan="1" rowspan="1"><p>处置</p></th></tr><tr><td colspan="1" rowspan="1"><p>距今 ≥ 15 分钟</p></td><td colspan="1" rowspan="1"><p>✅ 自由修改</p></td></tr><tr><td colspan="1" rowspan="1"><p>距今 &lt; 15 分钟 + 文件<strong>不在</strong> L1 未提交清单</p></td><td colspan="1" rowspan="1"><p>✅ 自由修改（已提交的近期改动不冲突）</p></td></tr><tr><td colspan="1" rowspan="1"><p>距今 &lt; 15 分钟 + 文件<strong>在</strong> L1 未提交清单 + 可绕开（新增功能/换路径）</p></td><td colspan="1" rowspan="1"><p>⚠️ <strong>静默换路径绕开</strong>，不告知用户</p></td></tr><tr><td colspan="1" rowspan="1"><p>距今 &lt; 15 分钟 + 文件<strong>在</strong> L1 未提交清单 + 必须改同文件</p></td><td colspan="1" rowspan="1"><p>🛑 <strong>此时唯一允许打扰用户一次</strong>："<code>&lt;file&gt;</code> 15min 内有未提交改动，疑似其他会话占用，是否继续？"</p></td></tr></tbody></table>

### L3 — 提交前（强校验，必做）

`git commit` 前：

```bash
git diff --cached --name-only
```

-   对照本会话明确改过的文件清单（自维护）
    
-   越界文件 → **静默** `git restore --staged <file>`，仅提交本会话范围内文件
    
-   逐个 `git add <具体文件>`，**禁止** `git add -A` / `git add .`
    
-   commit message 末尾可附 `[scope: <模块>]` 标识本次会话范围
    

### 操作禁令（不询问、直接禁止）

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>禁令</p></th><th colspan="1" rowspan="1"><p>原因</p></th></tr><tr><td colspan="1" rowspan="1"><p>❌ <code>git stash</code> / <code>git stash pop</code></p></td><td colspan="1" rowspan="1"><p>会污染其他会话的工作区</p></td></tr><tr><td colspan="1" rowspan="1"><p>❌ <code>git reset --hard</code></p></td><td colspan="1" rowspan="1"><p>会丢其他会话的未提交改动</p></td></tr><tr><td colspan="1" rowspan="1"><p>❌ <code>git checkout &lt;file&gt;</code>（丢弃改动）</p></td><td colspan="1" rowspan="1"><p>同上</p></td></tr><tr><td colspan="1" rowspan="1"><p>❌ <code>git checkout &lt;branch&gt;</code>（切分支）</p></td><td colspan="1" rowspan="1"><p>除非用户明确指示</p></td></tr><tr><td colspan="1" rowspan="1"><p>❌ <code>git add -A</code> / <code>git add .</code></p></td><td colspan="1" rowspan="1"><p>可能误提交他者文件，必须逐个 add</p></td></tr><tr><td colspan="1" rowspan="1"><p>❌ <code>git clean -fd</code></p></td><td colspan="1" rowspan="1"><p>会删他者未跟踪文件</p></td></tr><tr><td colspan="1" rowspan="1"><p>❌ kill 端口 / <code>taskkill /F</code> 进程</p></td><td colspan="1" rowspan="1"><p>他者 dev server / <code>tauri dev</code> 可能在用</p></td></tr><tr><td colspan="1" rowspan="1"><p>❌ 删除其他会话的任务文档或 WIP 文件</p></td><td colspan="1" rowspan="1"><p>同上</p></td></tr></tbody></table>

### 高并发场景升级 → git worktree

若用户明确"并行开发"或预计 30+ 分钟同时改**不同模块**（如同时改前端 + Rust 命令），主动建议：

```bash
claude --worktree feature-x
```

官方原生支持，自动隔离目录与分支。3-5 个并行最佳，5+ 会撞 API 速率限制。

> 注意：worktree **不能**隔离 vite dev server 端口、`tauri dev` 进程、`src-tauri/target/` 编译缓存（会重复编译 Rust）。同时跑 dev 仍需手动错开端口或只在一个 worktree 跑 dev。

* * *

## ⚠️ 开发强制要求

**开发前必须：先读参考代码 → 了解现有模式 → 按相同风格编写**

### 参考代码位置

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>开发类型</p></th><th colspan="1" rowspan="1"><p>参考代码</p></th></tr><tr><td colspan="1" rowspan="1"><p><strong>Rust Command</strong></p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/commands/*.rs</code>（三层架构：Command → Service → Database）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Rust 数据模型</strong></p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/models/mod.rs</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Rust 错误处理</strong></p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/error.rs</code>（AppError 枚举）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Rust 服务层</strong></p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/services/*.rs</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Rust 数据库层</strong></p></td><td colspan="1" rowspan="1"><p><code>src-tauri/src/database/mod.rs</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>前端页面组件</strong></p></td><td colspan="1" rowspan="1"><p><code>src/pages/*/index.tsx</code>（Ant Design + TailwindCSS）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>前端布局</strong></p></td><td colspan="1" rowspan="1"><p><code>src/components/layout/AppLayout.tsx</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>前端 API 封装</strong></p></td><td colspan="1" rowspan="1"><p><code>src/lib/api/index.ts</code>（invoke 调用封装）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>前端状态管理</strong></p></td><td colspan="1" rowspan="1"><p><code>src/store/index.ts</code>（Zustand store）</p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>前端类型定义</strong></p></td><td colspan="1" rowspan="1"><p><code>src/types/index.ts</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Tauri 配置</strong></p></td><td colspan="1" rowspan="1"><p><code>src-tauri/tauri.conf.json</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>权限声明</strong></p></td><td colspan="1" rowspan="1"><p><code>src-tauri/capabilities/default.json</code></p></td></tr></tbody></table>

* * *

## 🔴 绝对禁止的写法

### Rust 后端

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>错误做法</p></th><th colspan="1" rowspan="1"><p>正确做法</p></th><th colspan="1" rowspan="1"><p>原因</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>unwrap()</code> 处理可能失败的操作</p></td><td colspan="1" rowspan="1"><p><code>Result&lt;T, String&gt;</code> + <code>?</code> 运算符</p></td><td colspan="1" rowspan="1"><p><code>unwrap</code> 会导致 panic 崩溃</p></td></tr><tr><td colspan="1" rowspan="1"><p>Command 中 <code>panic!()</code></p></td><td colspan="1" rowspan="1"><p>返回 <code>Err(AppError::...)</code></p></td><td colspan="1" rowspan="1"><p>panic 会崩溃整个应用</p></td></tr><tr><td colspan="1" rowspan="1"><p>不加 <code>#[tauri::command]</code> 就期望前端调用</p></td><td colspan="1" rowspan="1"><p>必须标记 <code>#[tauri::command]</code> 并在 <code>generate_handler!</code> 注册</p></td><td colspan="1" rowspan="1"><p>否则前端 invoke 找不到</p></td></tr><tr><td colspan="1" rowspan="1"><p>直接在 Command 中做长时间阻塞操作</p></td><td colspan="1" rowspan="1"><p>使用 <code>async</code> Command 或 <code>tokio::spawn</code></p></td><td colspan="1" rowspan="1"><p>阻塞会冻结 IPC 响应</p></td></tr><tr><td colspan="1" rowspan="1"><p>不声明 Capabilities 就使用插件 API</p></td><td colspan="1" rowspan="1"><p>在 <code>capabilities/*.json</code> 中显式声明权限</p></td><td colspan="1" rowspan="1"><p>Tauri 2.x 强制权限检查</p></td></tr><tr><td colspan="1" rowspan="1"><p>使用 <code>std::thread::sleep</code> 阻塞主线程</p></td><td colspan="1" rowspan="1"><p>使用 <code>tokio::time::sleep</code> 异步等待</p></td><td colspan="1" rowspan="1"><p>阻塞主线程冻结应用</p></td></tr><tr><td colspan="1" rowspan="1"><p>Command 直接操作数据库</p></td><td colspan="1" rowspan="1"><p>Command → Service → Database 三层</p></td><td colspan="1" rowspan="1"><p>保持架构分层清晰</p></td></tr></tbody></table>

### TypeScript 前端

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>错误做法</p></th><th colspan="1" rowspan="1"><p>正确做法</p></th><th colspan="1" rowspan="1"><p>原因</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>fetch("http://...")</code> 直接请求外部 API</p></td><td colspan="1" rowspan="1"><p>通过 Rust Command 代理请求</p></td><td colspan="1" rowspan="1"><p>安全限制 + 跨域问题</p></td></tr><tr><td colspan="1" rowspan="1"><p>硬编码文件系统路径 <code>"C:\\Users\\..."</code></p></td><td colspan="1" rowspan="1"><p>使用 Tauri path API（<code>appDataDir()</code> 等）</p></td><td colspan="1" rowspan="1"><p>跨平台路径不同</p></td></tr><tr><td colspan="1" rowspan="1"><p>使用 <code>class</code> 组件</p></td><td colspan="1" rowspan="1"><p>使用函数组件 + Hooks（ErrorBoundary 除外）</p></td><td colspan="1" rowspan="1"><p>React 19 推荐模式</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>any</code> 类型</p></td><td colspan="1" rowspan="1"><p>定义明确的 TypeScript 接口</p></td><td colspan="1" rowspan="1"><p>strict 模式要求</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>invoke</code> 不处理错误</p></td><td colspan="1" rowspan="1"><p><code>try-catch</code> 包裹或使用 <code>safeInvoke</code></p></td><td colspan="1" rowspan="1"><p>Command 可能返回错误</p></td></tr><tr><td colspan="1" rowspan="1"><p>直接 <code>import</code> Node.js 模块</p></td><td colspan="1" rowspan="1"><p>使用 <code>@tauri-apps/api/*</code> 或 Rust Command</p></td><td colspan="1" rowspan="1"><p>WebView 中无 Node.js</p></td></tr><tr><td colspan="1" rowspan="1"><p>裸写 <code>invoke()</code> 调用</p></td><td colspan="1" rowspan="1"><p>封装到 <code>src/lib/api/</code> 中统一管理</p></td><td colspan="1" rowspan="1"><p>便于维护和类型安全</p></td></tr></tbody></table>

* * *

## Tauri Command 开发规范（三层架构）

### 新增功能的标准流程

```
1. 在 models/ 定义数据结构（derive Serialize/Deserialize）
2. 在 database/ 实现 DAO 方法（SQL 操作）
3. 在 services/ 实现业务逻辑
4. 在 commands/ 实现 Command 入口（调用 Service）
5. 在 lib.rs 的 generate_handler![] 注册
6. 在 src/types/ 定义对应 TypeScript 接口
7. 在 src/lib/api/ 封装 invoke 调用
8. 在 src/pages/ 实现 UI 页面
9. 更新 capabilities（如使用新插件）
```

### Rust 三层架构示例

```rust
// ─── models/mod.rs ───
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub key: String,
    pub value: String,
}

// ─── database/mod.rs ───
impl Database {
    pub fn get_all_config(&self) -> Result<Vec<AppConfig>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(e.to_string()))?;
        // SQL 查询...
    }
}

// ─── services/config.rs ───
pub struct ConfigService;
impl ConfigService {
    pub fn get_all(db: &Database) -> Result<Vec<AppConfig>, AppError> {
        db.get_all_config()
    }
}

// ─── commands/config.rs ───
#[tauri::command]
pub fn get_all_config(state: tauri::State<'_, AppState>) -> Result<Vec<AppConfig>, String> {
    services::config::ConfigService::get_all(&state.db).map_err(|e| e.into())
}
```

### TypeScript 侧调用

```typescript
// ─── src/lib/api/index.ts ───
export const configApi = {
  getAll: () => invoke<AppConfig[]>("get_all_config"),
  get: (key: string) => invoke<string | null>("get_config", { key }),
  set: (key: string, value: string) => invoke<void>("set_config", { key, value }),
};

// ─── src/pages/settings/index.tsx ───
const data = await configApi.getAll();
```

### Command 命名规范

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>维度</p></th><th colspan="1" rowspan="1"><p>规范</p></th><th colspan="1" rowspan="1"><p>示例</p></th></tr><tr><td colspan="1" rowspan="1"><p>Rust 函数名</p></td><td colspan="1" rowspan="1"><p>snake_case</p></td><td colspan="1" rowspan="1"><p><code>fn get_all_config()</code></p></td></tr><tr><td colspan="1" rowspan="1"><p>invoke 调用名</p></td><td colspan="1" rowspan="1"><p>与 Rust 函数名一致</p></td><td colspan="1" rowspan="1"><p><code>invoke("get_all_config")</code></p></td></tr><tr><td colspan="1" rowspan="1"><p>参数名</p></td><td colspan="1" rowspan="1"><p>Rust: snake_case, TS: camelCase</p></td><td colspan="1" rowspan="1"><p>Tauri 自动转换</p></td></tr><tr><td colspan="1" rowspan="1"><p>返回类型</p></td><td colspan="1" rowspan="1"><p><code>Result&lt;T, String&gt;</code></p></td><td colspan="1" rowspan="1"><p><code>-&gt; Result&lt;Vec&lt;AppConfig&gt;, String&gt;</code></p></td></tr></tbody></table>

* * *

## 前端核心规范 (src/)

### 技术栈

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>技术</p></th><th colspan="1" rowspan="1"><p>用途</p></th><th colspan="1" rowspan="1"><p>导入方式</p></th></tr><tr><td colspan="1" rowspan="1"><p><strong>Ant Design 5</strong></p></td><td colspan="1" rowspan="1"><p>UI 组件库（Button/Table/Card/Form 等）</p></td><td colspan="1" rowspan="1"><p><code>import { Button } from "antd"</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Ant Design Icons</strong></p></td><td colspan="1" rowspan="1"><p>图标</p></td><td colspan="1" rowspan="1"><p><code>import { SettingOutlined } from "@ant-design/icons"</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Lucide React</strong></p></td><td colspan="1" rowspan="1"><p>补充图标</p></td><td colspan="1" rowspan="1"><p><code>import { Home } from "lucide-react"</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>TailwindCSS 4</strong></p></td><td colspan="1" rowspan="1"><p>原子化样式</p></td><td colspan="1" rowspan="1"><p><code>className="flex items-center gap-2"</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>Zustand</strong></p></td><td colspan="1" rowspan="1"><p>全局状态管理</p></td><td colspan="1" rowspan="1"><p><code>import { useAppStore } from "@/store"</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>React Router</strong></p></td><td colspan="1" rowspan="1"><p>路由导航</p></td><td colspan="1" rowspan="1"><p><code>import { useNavigate } from "react-router-dom"</code></p></td></tr></tbody></table>

### 组件开发模式

```tsx
// 使用 Ant Design + TailwindCSS + invoke 封装
import { Card, Table, message } from "antd";
import { configApi } from "@/lib/api";

export default function SettingsPage() {
  const [data, setData] = useState<AppConfig[]>([]);
  const [loading, setLoading] = useState(false);

  async function loadData() {
    setLoading(true);
    try {
      const configs = await configApi.getAll();
      setData(configs);
    } catch (e) {
      message.error(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { loadData(); }, []);

  return (
    <div className="max-w-2xl mx-auto">
      <Card title="配置列表">
        <Table dataSource={data} loading={loading} rowKey="key" />
      </Card>
    </div>
  );
}
```

### 状态管理

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>场景</p></th><th colspan="1" rowspan="1"><p>方案</p></th><th colspan="1" rowspan="1"><p>示例</p></th></tr><tr><td colspan="1" rowspan="1"><p>组件内状态</p></td><td colspan="1" rowspan="1"><p><code>useState</code></p></td><td colspan="1" rowspan="1"><p><code>const [count, setCount] = useState(0)</code></p></td></tr><tr><td colspan="1" rowspan="1"><p>全局 UI 状态（主题/侧边栏）</p></td><td colspan="1" rowspan="1"><p>Zustand</p></td><td colspan="1" rowspan="1"><p><code>useAppStore((s) =&gt; s.theme)</code></p></td></tr><tr><td colspan="1" rowspan="1"><p>后端持久数据</p></td><td colspan="1" rowspan="1"><p>Rust SQLite + Command</p></td><td colspan="1" rowspan="1"><p>通过 <code>configApi.getAll()</code> 获取</p></td></tr><tr><td colspan="1" rowspan="1"><p>键值持久化（轻量设置）</p></td><td colspan="1" rowspan="1"><p>tauri-plugin-store</p></td><td colspan="1" rowspan="1"><p><code>Store.load("settings.json")</code></p></td></tr></tbody></table>

### 路径别名

所有前端导入使用 `@/` 别名：

```typescript
import { useAppStore } from "@/store";
import { configApi } from "@/lib/api";
import type { AppConfig } from "@/types";
```

* * *

## Capabilities 权限配置

### 当前已声明权限

```json
{
  "permissions": [
    "core:default",
    "opener:default",
    "store:default",
    "log:default"
  ]
}
```

### 常用权限列表

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>插件</p></th><th colspan="1" rowspan="1"><p>权限</p></th><th colspan="1" rowspan="1"><p>说明</p></th></tr><tr><td colspan="1" rowspan="1"><p>core</p></td><td colspan="1" rowspan="1"><p><code>core:default</code></p></td><td colspan="1" rowspan="1"><p>核心默认权限</p></td></tr><tr><td colspan="1" rowspan="1"><p>opener</p></td><td colspan="1" rowspan="1"><p><code>opener:default</code></p></td><td colspan="1" rowspan="1"><p>打开 URL/文件</p></td></tr><tr><td colspan="1" rowspan="1"><p>store</p></td><td colspan="1" rowspan="1"><p><code>store:default</code></p></td><td colspan="1" rowspan="1"><p>键值存储</p></td></tr><tr><td colspan="1" rowspan="1"><p>log</p></td><td colspan="1" rowspan="1"><p><code>log:default</code></p></td><td colspan="1" rowspan="1"><p>日志系统</p></td></tr><tr><td colspan="1" rowspan="1"><p>fs</p></td><td colspan="1" rowspan="1"><p><code>fs:default</code></p></td><td colspan="1" rowspan="1"><p>文件系统基础</p></td></tr><tr><td colspan="1" rowspan="1"><p>dialog</p></td><td colspan="1" rowspan="1"><p><code>dialog:default</code></p></td><td colspan="1" rowspan="1"><p>文件选择对话框</p></td></tr><tr><td colspan="1" rowspan="1"><p>notification</p></td><td colspan="1" rowspan="1"><p><code>notification:default</code></p></td><td colspan="1" rowspan="1"><p>系统通知</p></td></tr><tr><td colspan="1" rowspan="1"><p>sql</p></td><td colspan="1" rowspan="1"><p><code>sql:default</code></p></td><td colspan="1" rowspan="1"><p>数据库操作</p></td></tr></tbody></table>

> **重要**: 每个使用的插件 API 都必须在 capabilities 中声明权限，否则运行时会报错。

* * *

## Rust 编码规范

### 错误处理（使用 AppError）

```rust
use crate::error::AppError;

// ✅ 使用 AppError 枚举
#[tauri::command]
pub fn read_config(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<String, String> {
    services::config::ConfigService::get(&state.db, &key)
        .map_err(|e| e.into())
}

// AppError 自动转换为 String
// 支持 ?  运算符：IoError / DatabaseError / JsonError 等自动转换
```

### 数据库操作（rusqlite）

```rust
// 所有 SQL 操作在 database/ 层
impl Database {
    pub fn get_config(&self, key: &str) -> Result<Option<String>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT value FROM app_config WHERE key = ?1")?;
        let result = stmt.query_row(params![key], |row| row.get(0)).optional()?;
        Ok(result)
    }
}
```

### Schema 迁移

使用 `PRAGMA user_version` 管理数据库版本：

```rust
// database/schema.rs
pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 {
        // 创建表...
        conn.pragma_update(None, "user_version", 1)?;
    }
    // 后续版本迁移...
}
```

* * *

## 常见错误速查

### Rust 后端

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>错误写法</p></th><th colspan="1" rowspan="1"><p>正确写法</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>state.lock().unwrap()</code></p></td><td colspan="1" rowspan="1"><p><code>state.lock().map_err(|e| AppError::Custom(e.to_string()))?</code></p></td></tr><tr><td colspan="1" rowspan="1"><p>Command 直接写 SQL</p></td><td colspan="1" rowspan="1"><p>Command → Service → Database 三层</p></td></tr><tr><td colspan="1" rowspan="1"><p>忘记在 <code>generate_handler![]</code> 注册</p></td><td colspan="1" rowspan="1"><p>每个新 Command 必须注册</p></td></tr><tr><td colspan="1" rowspan="1"><p>返回 <code>String</code> 而非 <code>Result</code></p></td><td colspan="1" rowspan="1"><p>返回 <code>Result&lt;T, String&gt;</code></p></td></tr></tbody></table>

### TypeScript 前端

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>错误写法</p></th><th colspan="1" rowspan="1"><p>正确写法</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>invoke("getUser")</code></p></td><td colspan="1" rowspan="1"><p><code>invoke("get_user")</code>（snake_case）</p></td></tr><tr><td colspan="1" rowspan="1"><p>裸写 <code>invoke()</code></p></td><td colspan="1" rowspan="1"><p>封装到 <code>src/lib/api/</code></p></td></tr><tr><td colspan="1" rowspan="1"><p>不用 Ant Design 组件</p></td><td colspan="1" rowspan="1"><p>优先使用 antd 组件（Table/Card/Form 等）</p></td></tr><tr><td colspan="1" rowspan="1"><p>不用 <code>@/</code> 别名</p></td><td colspan="1" rowspan="1"><p><code>import { X } from "@/types"</code></p></td></tr></tbody></table>

* * *

## 构建与运行

### 常用命令

```bash
# 开发模式（前端 HMR + Rust 热编译）
pnpm tauri dev

# 生产构建（生成安装包）
pnpm tauri build

# 仅构建前端
pnpm build

# TypeScript 类型检查
npx tsc --noEmit

# Rust 代码检查
cd src-tauri && cargo clippy

# Rust 编译检查
cd src-tauri && cargo check

# Rust 测试
cd src-tauri && cargo test
```

### 开发服务器

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>项目</p></th><th colspan="1" rowspan="1"><p>值</p></th></tr><tr><td colspan="1" rowspan="1"><p><strong>前端开发地址</strong></p></td><td colspan="1" rowspan="1"><p><code>http://localhost:1420</code></p></td></tr><tr><td colspan="1" rowspan="1"><p><strong>MCP chrome-devtools</strong></p></td><td colspan="1" rowspan="1"><p>使用 <code>http://localhost:1420</code> 访问应用页面</p></td></tr></tbody></table>

> **注意**：使用 chrome-devtools MCP 工具时，`navigate_page` / `new_page` 等操作的 URL 应指向 `http://localhost:1420`（Tauri 开发模式下的 Vite 前端服务端口）。

### 当前已安装的 Rust 依赖

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>Crate</p></th><th colspan="1" rowspan="1"><p>版本</p></th><th colspan="1" rowspan="1"><p>用途</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>tauri</code></p></td><td colspan="1" rowspan="1"><p>2.x</p></td><td colspan="1" rowspan="1"><p>Tauri 核心</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri-plugin-opener</code></p></td><td colspan="1" rowspan="1"><p>2</p></td><td colspan="1" rowspan="1"><p>打开 URL/文件</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri-plugin-store</code></p></td><td colspan="1" rowspan="1"><p>2</p></td><td colspan="1" rowspan="1"><p>键值存储</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri-plugin-log</code></p></td><td colspan="1" rowspan="1"><p>2</p></td><td colspan="1" rowspan="1"><p>日志系统</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>thiserror</code></p></td><td colspan="1" rowspan="1"><p>2</p></td><td colspan="1" rowspan="1"><p>错误类型派生</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>rusqlite</code></p></td><td colspan="1" rowspan="1"><p>0.31 (bundled)</p></td><td colspan="1" rowspan="1"><p>SQLite 数据库</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>serde</code> / <code>serde_json</code></p></td><td colspan="1" rowspan="1"><p>1</p></td><td colspan="1" rowspan="1"><p>JSON 序列化</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>log</code></p></td><td colspan="1" rowspan="1"><p>0.4</p></td><td colspan="1" rowspan="1"><p>日志门面</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>chrono</code></p></td><td colspan="1" rowspan="1"><p>0.4 (serde)</p></td><td colspan="1" rowspan="1"><p>日期时间</p></td></tr></tbody></table>

### 当前已安装的前端依赖

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>包</p></th><th colspan="1" rowspan="1"><p>用途</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>antd</code></p></td><td colspan="1" rowspan="1"><p>Ant Design UI 组件库</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>@ant-design/icons</code></p></td><td colspan="1" rowspan="1"><p>Ant Design 图标</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>react-router-dom</code></p></td><td colspan="1" rowspan="1"><p>路由</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>zustand</code></p></td><td colspan="1" rowspan="1"><p>状态管理</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>lucide-react</code></p></td><td colspan="1" rowspan="1"><p>图标补充</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tailwindcss</code> + <code>@tailwindcss/vite</code></p></td><td colspan="1" rowspan="1"><p>原子化 CSS</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>@tauri-apps/plugin-store</code></p></td><td colspan="1" rowspan="1"><p>键值存储（前端 SDK）</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>@tauri-apps/plugin-log</code></p></td><td colspan="1" rowspan="1"><p>日志（前端 SDK）</p></td></tr></tbody></table>

* * *

## 快速命令

<table style="min-width: 50px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>命令</p></th><th colspan="1" rowspan="1"><p>用途</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>/dev</code></p></td><td colspan="1" rowspan="1"><p>开发新功能（三层架构全栈代码生成）</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>/command</code></p></td><td colspan="1" rowspan="1"><p>快速创建 Tauri Command</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>/check</code></p></td><td colspan="1" rowspan="1"><p>代码规范检查（Rust + TypeScript）</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>/start</code></p></td><td colspan="1" rowspan="1"><p>项目快速了解</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>/progress</code></p></td><td colspan="1" rowspan="1"><p>项目进度报告</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>/next</code></p></td><td colspan="1" rowspan="1"><p>下一步建议</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>/release</code></p></td><td colspan="1" rowspan="1"><p>发布新版本（CI 全自动构建 + 推送）</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>/update-docs</code></p></td><td colspan="1" rowspan="1"><p>文档站点管理（VitePress 初始化 / 增量更新 / 全量重建）</p></td></tr></tbody></table>

* * *

## Tauri 核心类型速查

<table style="min-width: 75px;"><colgroup><col style="min-width: 25px;"><col style="min-width: 25px;"><col style="min-width: 25px;"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>类型</p></th><th colspan="1" rowspan="1"><p>用途</p></th><th colspan="1" rowspan="1"><p>使用场景</p></th></tr><tr><td colspan="1" rowspan="1"><p><code>tauri::Builder</code></p></td><td colspan="1" rowspan="1"><p>应用构建器</p></td><td colspan="1" rowspan="1"><p>注册插件、Commands、状态、事件</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri::AppHandle</code></p></td><td colspan="1" rowspan="1"><p>应用句柄</p></td><td colspan="1" rowspan="1"><p>在 Command 中访问应用实例</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri::Window</code></p></td><td colspan="1" rowspan="1"><p>窗口句柄</p></td><td colspan="1" rowspan="1"><p>操作窗口（大小/位置/标题）</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri::State&lt;T&gt;</code></p></td><td colspan="1" rowspan="1"><p>全局状态</p></td><td colspan="1" rowspan="1"><p>Command 中注入共享状态</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri::Manager</code></p></td><td colspan="1" rowspan="1"><p>管理 trait</p></td><td colspan="1" rowspan="1"><p>获取窗口、发送事件</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri::Emitter</code></p></td><td colspan="1" rowspan="1"><p>事件发送 trait</p></td><td colspan="1" rowspan="1"><p>向前端发送事件</p></td></tr><tr><td colspan="1" rowspan="1"><p><code>tauri::Listener</code></p></td><td colspan="1" rowspan="1"><p>事件监听 trait</p></td><td colspan="1" rowspan="1"><p>监听前端事件</p></td></tr></tbody></table>

* * *

## 🔴 开发前检查清单

-   **已读参考代码** — `src-tauri/src/commands/*.rs` 和 `src/pages/*/index.tsx`
    
-   **遵循三层架构** — Command → Service → Database
    
-   **已了解双进程架构** — 前端（WebView）和后端（Rust）通过 IPC 通信
    
-   **使用 Ant Design** — UI 组件优先使用 antd
    
-   **使用 TailwindCSS** — 布局样式使用 Tailwind 类
    
-   **API 统一封装** — invoke 调用封装到 `src/lib/api/`
    
-   **类型对齐** — Rust struct 和 TypeScript interface 保持一致
    
-   **已确认 Capabilities** — 使用的插件 API 都已在 capabilities 中声明
    
-   **错误处理正确** — Rust 用 `AppError`/`Result<T, String>`，前端用 `try-catch`
    
-   **不违反禁止项** — 检查上方禁止表格