# AI 会话树改进方案

> 在 Deepseek-TUI 插件基础上，增加会话属性（工作目录、类型）与会话结构树。

---

## 1. 需求概述

| 需求 | 说明 |
|------|------|
| 新建会话按钮 | 在会话树上方放置醒目的"新建会话"按钮，弹出属性对话框 |
| 会话属性 | 工作目录（可选）、名称、类型（6 种） |
| 会话类型 | 研究探索、方案设计、开发任务、编程实现、测试验证、归档存储 |
| 结构树 | 左侧面板以树形展示：工作目录为父节点，会话为叶节点 |

---

## 2. 会话类型定义

| 类型标识 | 中文名称 | 图标 | 说明 |
|---------|---------|------|------|
| `research` | 研究探索 | `Search` | 技术调研、资料收集、可行性分析 |
| `design` | 方案设计 | `Lightbulb` | 架构设计、方案对比、技术决策 |
| `development` | 开发任务 | `Code2` | 功能开发、Bug 修复、重构 |
| `programming` | 编程实现 | `Terminal` | 具体编码、算法实现 |
| `testing` | 测试验证 | `CheckCircle2` | 单元测试、集成测试、QA |
| `archive` | 归档存储 | `Archive` | 已完成对话的归档保存 |

---

## 3. 数据库变更

### 3.1 Schema v38 → v39

```sql
ALTER TABLE ai_conversations ADD COLUMN work_directory TEXT NOT NULL DEFAULT '';

ALTER TABLE ai_conversations ADD COLUMN session_type TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_ai_conv_work_dir
    ON ai_conversations(work_directory)
    WHERE work_directory != '';
```

### 3.2 字段说明

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `work_directory` | TEXT | `""` | 工作目录路径，空字符串表示"默认"分组 |
| `session_type` | TEXT | `""` | 会话类型标识，空字符串表示未分类 |

### 3.3 完整表结构（变更后）

```sql
CREATE TABLE ai_conversations (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    title               TEXT NOT NULL DEFAULT '新对话',
    model_id            INTEGER NOT NULL REFERENCES ai_models(id) ON DELETE CASCADE,
    attached_note_ids   TEXT NOT NULL DEFAULT '[]',
    work_directory      TEXT NOT NULL DEFAULT '',
    session_type        TEXT NOT NULL DEFAULT '',
    created_at          TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
    updated_at          TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);
```

---

## 4. Rust 后端变更

### 4.1 models/mod.rs — 数据模型

```rust
/// 会话类型枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionType {
    #[serde(rename = "research")]
    Research,
    #[serde(rename = "design")]
    Design,
    #[serde(rename = "development")]
    Development,
    #[serde(rename = "programming")]
    Programming,
    #[serde(rename = "testing")]
    Testing,
    #[serde(rename = "archive")]
    Archive,
}

impl std::fmt::Display for SessionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Research => write!(f, "research"),
            Self::Design => write!(f, "design"),
            Self::Development => write!(f, "development"),
            Self::Programming => write!(f, "programming"),
            Self::Testing => write!(f, "testing"),
            Self::Archive => write!(f, "archive"),
        }
    }
}

// ═══ AiConversation 新增字段 ═══
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConversation {
    pub id: i64,
    pub title: String,
    pub model_id: i64,
    pub attached_note_ids: Vec<i64>,
    pub work_directory: String,    // 新增
    pub session_type: String,      // 新增，空串表示未指定类型
    pub created_at: String,
    pub updated_at: String,
}

// ═══ 创建对话入参（扩展） ═══
#[derive(Debug, Clone, Deserialize)]
pub struct CreateConversationInput {
    pub title: Option<String>,
    pub model_id: Option<i64>,
    pub work_directory: Option<String>,
    pub session_type: Option<String>,
}

// ═══ 更新对话属性入参 ═══
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateConversationInput {
    pub title: Option<String>,
    pub model_id: Option<i64>,
    pub work_directory: Option<String>,
    pub session_type: Option<String>,
}

// ═══ 会话树节点（用于前端渲染） ═══
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTreeNode {
    /// "directory" | "conversation"
    pub node_type: String,
    /// 节点显示名
    pub label: String,
    /// 工作目录路径（仅 directory 节点有效）
    pub work_directory: Option<String>,
    /// 会话信息（仅 conversation 节点有效）
    pub conversation: Option<AiConversation>,
    /// 子节点（仅 directory 节点有效）
    pub children: Vec<ConversationTreeNode>,
}
```

### 4.2 database/ai.rs — 数据库方法

```rust
/// AI_CONV_COLS 常量更新
const AI_CONV_COLS: &str =
    "id, title, model_id, attached_note_ids, work_directory, session_type, created_at, updated_at";

/// row_to_ai_conversation 更新
fn row_to_ai_conversation(row: &rusqlite::Row) -> rusqlite::Result<AiConversation> {
    let attached_json: String = row.get(3)?;
    let attached_note_ids: Vec<i64> =
        serde_json::from_str(&attached_json).unwrap_or_default();
    Ok(AiConversation {
        id: row.get(0)?,
        title: row.get(1)?,
        model_id: row.get(2)?,
        attached_note_ids,
        work_directory: row.get(4)?,
        session_type: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// 创建对话（扩展参数）
pub fn create_ai_conversation(
    &self,
    title: &str,
    model_id: i64,
    work_directory: &str,
    session_type: &str,
) -> Result<AiConversation, AppError> {
    let conn = self.conn.lock().map_err(|e| AppError::Custom(e.to_string()))?;
    conn.execute(
        "INSERT INTO ai_conversations (title, model_id, work_directory, session_type)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![title, model_id, work_directory, session_type],
    )?;
    let id = conn.last_insert_rowid();
    let sql = format!("SELECT {} FROM ai_conversations WHERE id = ?1", AI_CONV_COLS);
    let conv = conn.query_row(&sql, [id], row_to_ai_conversation)?;
    Ok(conv)
}

/// 更新对话属性
pub fn update_ai_conversation(
    &self,
    id: i64,
    input: &UpdateConversationInput,
) -> Result<AiConversation, AppError> {
    let conn = self.conn.lock().map_err(|e| AppError::Custom(e.to_string()))?;
    // 动态构建 SET 子句
    let mut sets = vec!["updated_at = datetime('now', 'localtime')".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];
    if let Some(t) = &input.title {
        sets.push(format!("title = ?{}", params.len() + 1));
        params.push(Box::new(t.clone()));
    }
    if let Some(m) = input.model_id {
        sets.push(format!("model_id = ?{}", params.len() + 1));
        params.push(Box::new(m));
    }
    if let Some(w) = &input.work_directory {
        sets.push(format!("work_directory = ?{}", params.len() + 1));
        params.push(Box::new(w.clone()));
    }
    if let Some(s) = &input.session_type {
        sets.push(format!("session_type = ?{}", params.len() + 1));
        params.push(Box::new(s.clone()));
    }
    let sql = format!(
        "UPDATE ai_conversations SET {} WHERE id = ?{}",
        sets.join(", "),
        params.len() + 1
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.execute(rusqlate::params_from_iter(params.iter().map(|p| p.as_ref())))?;
    // 重新查询
    let sql = format!("SELECT {} FROM ai_conversations WHERE id = ?1", AI_CONV_COLS);
    conn.query_row(&sql, [id], row_to_ai_conversation)
        .map_err(AppError::from)
}

/// 获取所有工作目录列表（去重）
pub fn list_work_directories(&self) -> Result<Vec<String>, AppError> {
    let conn = self.conn.lock().map_err(|e| AppError::Custom(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT work_directory FROM ai_conversations
         WHERE work_directory != ''
         ORDER BY work_directory",
    )?;
    let dirs = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(dirs)
}
```

### 4.3 commands/ai.rs — IPC 入口

```rust
/// 创建对话（扩展入参）
#[tauri::command]
pub fn create_ai_conversation(
    state: State<'_, AppState>,
    input: CreateConversationInput,
) -> Result<AiConversation, String> {
    let title = input.title.unwrap_or_else(|| "新对话".to_string());
    let model_id = match input.model_id {
        Some(id) => id,
        None => state.db.get_default_ai_model().map_err(|e| e.to_string())?.id,
    };
    let work_directory = input.work_directory.unwrap_or_default();
    let session_type = input.session_type.unwrap_or_default();
    state
        .db
        .create_ai_conversation(&title, model_id, &work_directory, &session_type)
        .map_err(|e| e.to_string())
}

/// 更新对话属性
#[tauri::command]
pub fn update_ai_conversation(
    state: State<'_, AppState>,
    id: i64,
    input: UpdateConversationInput,
) -> Result<AiConversation, String> {
    state
        .db
        .update_ai_conversation(id, &input)
        .map_err(|e| e.to_string())
}

/// 获取会话树（前端直接渲染用）
#[tauri::command]
pub fn get_conversation_tree(state: State<'_, AppState>) -> Result<Vec<ConversationTreeNode>, String> {
    let convs = state.db.list_ai_conversations().map_err(|e| e.to_string())?;
    let mut dirs: BTreeMap<String, Vec<AiConversation>> = BTreeMap::new();

    for conv in convs {
        let key = if conv.work_directory.is_empty() {
            "(默认)".to_string()
        } else {
            conv.work_directory.clone()
        };
        dirs.entry(key).or_default().push(conv);
    }

    let nodes: Vec<ConversationTreeNode> = dirs
        .into_iter()
        .map(|(dir, convs)| {
            let children: Vec<ConversationTreeNode> = convs
                .into_iter()
                .map(|c| ConversationTreeNode {
                    node_type: "conversation".to_string(),
                    label: c.title.clone(),
                    work_directory: None,
                    conversation: Some(c),
                    children: vec![],
                })
                .collect();
            ConversationTreeNode {
                node_type: "directory".to_string(),
                label: dir.clone(),
                work_directory: if dir == "(默认)" { Some("".to_string()) } else { Some(dir) },
                conversation: None,
                children,
            }
        })
        .collect();

    Ok(nodes)
}

/// 获取所有工作目录（供新建会话时选择已有目录）
#[tauri::command]
pub fn list_work_directories(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.db.list_work_directories().map_err(|e| e.to_string())
}
```

### 4.4 lib.rs — 注册新 Command

```rust
.invoke_handler(tauri::generate_handler![
    // ... 已有 commands ...
    commands::ai::get_conversation_tree,       // 新增
    commands::ai::list_work_directories,        // 新增
    commands::ai::update_ai_conversation,       // 新增（修改已有 create 签名）
])
```

---

## 5. TypeScript 类型定义

### 5.1 types/index.ts — 新增类型

```typescript
/** 会话类型枚举 */
export type SessionType =
  | "research"      // 研究探索
  | "design"        // 方案设计
  | "development"   // 开发任务
  | "programming"   // 编程实现
  | "testing"       // 测试验证
  | "archive";      // 归档存储

/** 会话类型选项（供 Select 使用） */
export const SESSION_TYPE_OPTIONS: { value: SessionType; label: string; icon: string }[] = [
  { value: "research",      label: "研究探索", icon: "Search" },
  { value: "design",        label: "方案设计", icon: "Lightbulb" },
  { value: "development",   label: "开发任务", icon: "Code2" },
  { value: "programming",   label: "编程实现", icon: "Terminal" },
  { value: "testing",       label: "测试验证", icon: "CheckCircle2" },
  { value: "archive",       label: "归档存储", icon: "Archive" },
];

/** 会话树节点 */
export interface ConversationTreeNode {
  nodeType: "directory" | "conversation";
  label: string;
  workDirectory: string | null;
  conversation: AiConversation | null;
  children: ConversationTreeNode[];
}

/** 扩展 AiConversation */
export interface AiConversation {
  id: number;
  title: string;
  modelId: number;
  attachedNoteIds: number[];
  workDirectory: string;   // 新增
  sessionType: string;     // 新增
  createdAt: string;
  updatedAt: string;
}

/** 创建对话入参 */
export interface CreateConversationInput {
  title?: string;
  modelId?: number;
  workDirectory?: string;
  sessionType?: string;
}

/** 更新对话入参 */
export interface UpdateConversationInput {
  title?: string;
  modelId?: number;
  workDirectory?: string;
  sessionType?: string;
}
```

---

## 6. 插件前端改造

### 6.1 改造范围

| 文件 | 改动 |
|------|------|
| `dev-plugins/deepseek-tui/plugin.json` | 无改动 |
| `dev-plugins/deepseek-tui/main.js` | 添加新建会话对话框、改造树形列表渲染 |
| `dev-plugins/deepseek-tui/styles.css` | 添加对话框、树形节点样式 |

### 6.2 左侧面板 UI 设计

```
┌────────────────────────────┐
│  会话                 [+]  │  ← 标题 + 新建按钮
├────────────────────────────┤
│                            │
│  📁 D:/projects/notebook   │  ← 工作目录节点（折叠/展开）
│    ├── 🗨️ 架构设计方案       │  ← 会话叶节点（方案设计图标）
│    ├── 🗨️ API 重构讨论       │
│    └── 🗨️ 性能优化分析       │
│                            │
│  📁 D:/projects/cli-tool   │
│    ├── 🗨️ Rust CLI 实现      │
│    └── 🗨️ 单元测试编写       │
│                            │
│  📁 (默认)                  │  ← 无工作目录的会话分组
│    └── 🗨️ 日常问答           │
│                            │
└────────────────────────────┘
```

### 6.3 新建会话对话框

```
┌──────────────────────────────────────────┐
│  新建 Deepseek 会话                  [✕] │
├──────────────────────────────────────────┤
│                                          │
│  名称 *                                  │
│  ┌──────────────────────────────────────┐│
│  │ 请输入会话名称                        ││
│  └──────────────────────────────────────┘│
│                                          │
│  类型                                    │
│  ┌──────────────────────────────────────┐│
│  │ 🔍 研究探索            ▼            ││
│  │   🔍 研究探索 - 技术调研、可行性分析  ││
│  │   💡 方案设计 - 架构、技术决策       ││
│  │   💻 开发任务 - 功能开发、重构       ││
│  │   ⌨️  编程实现 - 具体编码、算法       ││
│  │   ✅ 测试验证 - 测试、QA             ││
│  │   📦 归档存储 - 已完成对话归档       ││
│  └──────────────────────────────────────┘│
│                                          │
│  工作目录 (可选)                         │
│  ┌──────────────────────────────────────┐│
│  │ 选择或输入工作目录路径        [浏览] ││
│  └──────────────────────────────────────┘│
│                                          │
│  AI 模型                                 │
│  ┌──────────────────────────────────────┐│
│  │ Deepseek-V3 (默认)          ▼       ││
│  └──────────────────────────────────────┘│
│                                          │
├──────────────────────────────────────────┤
│                     [取消]    [创建会话]  │
└──────────────────────────────────────────┘
```

### 6.4 main.js 改造要点

```js
// ═══ 新建会话对话框 ═══
function showNewSessionDialog(app) {
  // 读取工作目录列表 + AI 模型列表
  var dirs = await app.invoke("list_work_directories");
  var models = await app.invoke("list_ai_models");

  // 渲染对话框 DOM
  var dialog = createDialog({
    title: "新建 Deepseek 会话",
    fields: [
      { type: "input", key: "title", label: "名称", required: true,
        placeholder: "请输入会话名称" },
      { type: "select", key: "sessionType", label: "类型",
        options: SESSION_TYPES },
      { type: "autocomplete", key: "workDirectory", label: "工作目录",
        options: dirs, allowCustom: true },
      { type: "select", key: "modelId", label: "AI 模型",
        options: models, defaultLabel: "默认模型" },
    ],
    onConfirm: async function (values) {
      var conv = await app.invoke("create_ai_conversation", {
        input: {
          title: values.title,
          workDirectory: values.workDirectory,
          sessionType: values.sessionType,
          modelId: values.modelId,
        },
      });
      renderConversationTree();
      DeepseekAPI.switchConversation(conv.id);
    },
  });
}

// ═══ 树形列表渲染 ═══
async function renderConversationTree() {
  var tree = await app.invoke("get_conversation_tree");
  sidebarList.innerHTML = "";

  tree.forEach(function (dirNode) {
    // 目录节点（可折叠）
    var dirEl = createDirNode(dirNode);
    sidebarList.appendChild(dirEl);

    // 会话叶节点
    dirNode.children.forEach(function (convNode) {
      var convEl = createConvNode(convNode, dirNode);
      sidebarList.appendChild(convEl);
    });
  });
}
```

---

## 7. ICON 映射（会话类型 → 图标）

| session_type | 图标组件 | CSS 颜色 |
|-------------|---------|----------|
| `research` | `<Search>` | `#58a6ff` (蓝) |
| `design` | `<Lightbulb>` | `#d2a700` (金) |
| `development` | `<Code2>` | `#3fb950` (绿) |
| `programming` | `<Terminal>` | `#f78166` (橙) |
| `testing` | `<CheckCircle2>` | `#bc8cff` (紫) |
| `archive` | `<Archive>` | `#8b949e` (灰) |
| 未指定 | `<MessageSquare>` | `#484f58` (暗灰) |

---

## 8. 实施步骤

| 步骤 | 文件 | 内容 | 预估 |
|------|------|------|------|
| **1. DB 迁移** | `database/schema.rs` | v38→v39: ALTER TABLE 新增 `work_directory`、`session_type` | 15 行 |
| **2. 模型更新** | `models/mod.rs` | AiConversation 新字段、CreateConversationInput、UpdateConversationInput、ConversationTreeNode | ~60 行 |
| **3. DB 方法** | `database/ai.rs` | update create_ai_conversation、新增 update/list_dirs、更新 row mapper | ~50 行 |
| **4. Commands** | `commands/ai.rs` | 更新 create、新增 update/get_tree/list_dirs | ~40 行 |
| **5. 注册** | `lib.rs` | generate_handler! 新增 3 个 command | 3 行 |
| **6. TS 类型** | `types/index.ts` | SessionType、SESSION_TYPE_OPTIONS、ConversationTreeNode 等 | ~40 行 |
| **7. 插件 API 层** | `main.js` (api 部分) | 新增 getConversationTree、listWorkDirectories、update 调用 | ~30 行 |
| **8. 对话框 UI** | `main.js` (UI 部分) | createNewSessionDialog (Modal + Form) | ~120 行 |
| **9. 树形列表** | `main.js` (渲染部分) | renderConversationTree (折叠/展开/图标) | ~80 行 |
| **10. 样式** | `styles.css` | 树节点、对话框样式 | ~60 行 |
| **总计** | | | **~498 行，1-2 天** |

---

## 9. 兼容性说明

- `work_directory` 和 `session_type` 默认空字符串，**完全向后兼容**现有数据
- 现有 `create_ai_conversation` 参数改为 `CreateConversationInput` 对象，前端调用方式需更新
- 旧插件调用 `invoke("create_ai_conversation", { title, modelId })` 会失败，需同步更新 `dev-plugins/deepseek-tui/main.js`
- 插件权限无需新增（`ai:chat`、`ai:models` 已覆盖）
