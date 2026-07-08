# AI 助学接入与后续开发说明

本文档记录当前 AI 助学 MVP 已完成的接入内容，以及后续继续扩展数据库、题库、评分和进度记录时应该改哪里。

## 当前目标

当前版本只做最小可运行 MVP：

```text
目标解析 -> 计划生成 -> 阶段任务展示
```

完整闭环预留为：

```text
目标解析 -> 计划生成 -> 阶段任务 -> 资源推荐 -> 成果检查 -> 进度记录 -> 计划调整
```

其中资源推荐、测试、评分、进度记录、计划调整目前只保留入口提示，尚未接入真实数据库和题库。

## 新增目录

### `learning-assistant/`

AI 助学独立能力目录，和 `ppt-master/` 的组织思路一致。它不放在 `Pomegranate/src` 内，而是作为可独立维护的助学能力文件夹存在。

重要文件：

```text
learning-assistant/
├─ README.md
├─ templates/
│  └─ plan_template.json
└─ skills/
   └─ learning-assistant/
      ├─ SKILL.md
      ├─ workflows/
      │  └─ generate-learning-plan.md
      └─ references/
         ├─ planning-rules.md
         └─ scoring-rules.md
```

作用：

- `SKILL.md`：说明 AI 助学 skill 的目标、输入、输出和工作流。
- `generate-learning-plan.md`：说明初版学习计划生成流程。
- `planning-rules.md`：规定计划生成必须包含 3-5 个阶段及阶段字段。
- `scoring-rules.md`：预留后续测试评分和计划调整规则。
- `plan_template.json`：描述结构化学习计划输出形态。

## 新增前端页面

### `Pomegranate/src/pages/learning-assistant/index.tsx`

AI 助学主页面。

当前支持：

- 输入学习目标
- 填写课程名称、学习周期、每日学习时间、当前基础、最终目标
- 点击 `AI 理解目标` 生成结构化目标理解
- 点击 `生成学习计划` 生成 4 个阶段任务
- 每个阶段展示学习任务、资源任务、练习任务、检验任务、完成标准
- 预留 `推荐资源`、`开始测试`、`调整计划` 三个按钮
- 浏览器模式 fallback：不依赖 Tauri `invoke`，可直接模拟生成计划
- Tauri 模式优先调用后端 command，失败时回退到前端模拟生成，方便调试

后续扩展建议：

- 如果接入真实 AI，把 `callLearningAssistant` 里的 fallback 和 Tauri command 返回结构保持一致。
- 如果接入资源库，把 `推荐资源` 按钮改成调用后端资源推荐 command。
- 如果接入题库，把 `开始测试` 按钮改成打开测试窗口或路由。
- 如果接入学习记录，把 `调整计划` 按钮改成读取阶段完成度和测试分数后生成新计划。

## 修改的前端入口

### `Pomegranate/src/Router.tsx`

新增路由：

```text
/learning-assistant
```

作用：让用户能通过 URL 或菜单进入 AI 助学页面。

### `Pomegranate/src/components/layout/ActivityBar.tsx`

新增左侧主功能入口：

```text
AI 助学
```

作用：让 AI 助学像 AI PPT 一样出现在左侧功能区。

### `Pomegranate/src/components/layout/Sidebar.tsx`

新增侧边栏导航项：

```text
AI 助学
```

作用：让页面在侧边栏中也能被访问。

### `Pomegranate/src/store/index.ts`

新增 active view：

```text
learning-assistant
```

作用：让布局状态能识别当前功能视图。

## 新增 Tauri 后端 command

### `Pomegranate/src-tauri/src/commands/learning_assistant.rs`

提供前端可调用的 Tauri command：

- `learning_assistant_check`
- `learning_assistant_understand`
- `learning_assistant_generate_plan`

作用：作为前端和 Rust service 的 IPC 边界。

### `Pomegranate/src-tauri/src/services/learning_assistant.rs`

AI 助学后端 service。

当前职责：

- 检查 `learning-assistant/` 目录是否完整
- 检查 skill、workflow、rules、template 文件是否存在
- 根据用户输入生成结构化目标理解
- 生成 4 个阶段的学习计划
- 为后续真实 AI、数据库、题库、评分逻辑预留 service 层入口

后续扩展建议：

- 真实 AI：在这里接入统一 AI 服务，而不是把 prompt 写死在页面里。
- 数据库：在这里读取课程资源、题库、历史学习记录。
- 评分：按 `scoring-rules.md` 根据测试分数决定是否进入下一阶段。
- 计划调整：根据进度记录和评分结果重新生成后续阶段任务。

## 修改的 Tauri 注册文件

### `Pomegranate/src-tauri/src/commands/mod.rs`

注册 command 模块：

```rust
pub mod learning_assistant;
```

### `Pomegranate/src-tauri/src/services/mod.rs`

注册 service 模块：

```rust
pub mod learning_assistant;
```

### `Pomegranate/src-tauri/src/lib.rs`

把 AI 助学 command 加入 Tauri invoke handler。

作用：前端才能通过 `invoke("learning_assistant_generate_plan", ...)` 调用 Rust 后端。

## 当前 MVP 的调用方式

浏览器调试：

```text
React 页面 -> 前端模拟生成
```

Tauri 桌面调试：

```text
React 页面 -> Tauri invoke -> commands/learning_assistant.rs -> services/learning_assistant.rs -> learning-assistant/ skill 文件检查与计划生成
```

## 后续开发路线

### 第一步：接入真实 AI 生成

修改：

```text
Pomegranate/src-tauri/src/services/learning_assistant.rs
```

建议增加：

- prompt 构造函数
- AI provider 调用函数
- JSON schema 校验或结果修复函数

保持前端返回结构不变：

```text
LearningAssistantPlanResult
```

### 第二步：接入资源数据库

建议新增：

```text
Pomegranate/src-tauri/src/commands/learning_resources.rs
Pomegranate/src-tauri/src/services/learning_resources.rs
```

或在学习助学 service 中先做 MVP。

资源推荐按钮后续调用资源推荐 command。

### 第三步：接入题库和阶段测试

建议新增：

```text
Pomegranate/src/pages/learning-assistant/test.tsx
Pomegranate/src-tauri/src/commands/learning_quiz.rs
Pomegranate/src-tauri/src/services/learning_quiz.rs
```

`开始测试` 按钮进入测试页或弹窗。

### 第四步：接入进度记录

建议新增本地数据表或存储结构：

```text
learning_plan
learning_stage
learning_progress
learning_test_result
```

`调整计划` 根据进度记录和评分结果重新生成计划。

### 第五步：按评分规则调整计划

参考：

```text
learning-assistant/skills/learning-assistant/references/scoring-rules.md
```

规则：

- 85 分及以上：进入下一阶段，可增加提高任务
- 70-84 分：进入下一阶段，但增加薄弱知识点复习
- 60-69 分：减少下一阶段新内容，安排补弱资源和基础练习
- 60 分以下：重学本阶段，降低难度，重新测试后再推进

## 交付注意事项

交给别人继续开发时，请打包整个 `pomegranate-ai-ppt/` 总目录，不要只打包 `Pomegranate/`。

必须保留：

```text
Pomegranate/
learning-assistant/
ppt-master/
项目调试启动说明.md
AI助学接入与后续开发说明.md
```

