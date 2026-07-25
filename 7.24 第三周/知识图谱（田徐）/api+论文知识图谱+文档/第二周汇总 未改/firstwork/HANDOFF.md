# firstwork 项目交接说明

更新时间：2026-07-19

## 1. 项目定位

`D:\ag\firstwork` 是当前唯一主版本。此前多个同学分支中的有效功能已经按语义合并进该目录，后续开发、测试和上传仓库都应以 `firstwork` 为准。

不要再依赖或引用这些历史参考目录：


## 2. 当前主要能力

### Pomegranate 桌面主程序

- Tauri + React + TypeScript 桌面应用。
- 包管理器使用 `pnpm`。
- Rust 后端位于 `Pomegranate/src-tauri`。
- 前端入口位于 `Pomegranate/src`。
- 当前侧边栏入口较多，已支持滚动访问。

### AI 助学

- 已合入学生端 AI 助学链路。
- 支持学习目标理解、计划生成、本地知识库搜索、阶段资源推荐、正式题库取题、评分、薄弱点、缺失关键词、错题复盘、多学习项目保存和恢复。
- 学生端正式题库路径：
  `files.v21_最终/question_bank_system/db/question_bank.db`
- 知识库与模板路径：
  `learning-assistant/data/`

### 题库生产工具

- 独立放置于 `question-bank-tooling/`。
- 与学生端正式题库隔离。
- 导入、重建、审核等会修改数据库的操作默认不自动执行，应使用临时开发库或副本。
- 不要用生产工具直接覆盖学生端正式库。

### AI 助研

- 已从旧版副本中整合进当前主版本。
- 入口、页面和必要调用链已并入当前 Pomegranate。
- 数据应与 AI 助学、笔记、课程图谱等模块隔离。

### PPT 生成

- PPT 生成继续复用 `ppt-master`，Pomegranate 负责 UI、配置、理解和调用链。
- 已支持短素材 direct 理解。
- 已支持长素材分块理解、失败重试、合并理解。
- 已接入 Zhuai 的 Native Quality 可选质量链：
  Planning / Theme / Density。
- Native Quality 当前建议作为可选功能保留，默认关闭。
- 最近一次严格 A/B 验收产物在：
  `.runtime-data-final/ppt-native-quality-strict-ab/`
  该目录是测试产物，不应提交仓库。

### 课程知识图谱

- 机械制造工艺课程图谱已从 Neo4j/FastAPI 运行链转为本地 SQLite 资源。
- 数据库资源：
  `Pomegranate/src-tauri/resources/process_graph.db`
- 前端继续使用课程知识图谱独立页面和 Cytoscape 展示。
- 普通运行不再依赖 Docker、Neo4j、Java、Python 或 FastAPI。
- 不要与原有笔记双向链接图混淆。

### AI 应用市场、插件、开发者中心、审核中心

- 已有本地模拟市场、开发者中心、审核中心、插件安装和声明式插件体系。
- 已实现 AI 文档摘要插件和部分插件生命周期验证。
- 当前市场交易仍为本地模拟，不接真实支付、余额、提现或分账。
- 星辰工作流商品采用 BYOK 或外部授权思路，不能把开发者密钥分发给用户。
- 开发者中心和插件调用相关能力仍需要继续人工验收。

### AI 资源中心与星辰 Workflow

- 已支持 BYOK 凭据管理。
- API Key、API Secret 等敏感信息应由 Rust 后端安全保存，不应进入前端、日志或普通 SQLite 明文字段。
- 已接入讯飞星辰 Workflow Open API v1。
- 支持通用工作流字段配置：不同工作流可以配置不同开始节点输入字段，动态生成 parameters。
- 如果星辰返回 `20354`，通常是工作流开始节点字段名或参数 schema 不匹配，应在 AI 资源中心的智能体配置里核对字段 key。

### 文档导入

- 已增强 Markdown、Word、PDF、混合文件夹批量导入。
- PDF 当前偏向可编辑笔记模式；原文档查看与图片显示仍需要继续完善和人工验收。

## 3. 重要目录说明

必须保留并随项目交付的内容：

- `Pomegranate/`
- `ppt-master/`
- `learning-assistant/`
- `files.v21_最终/`
- `question-bank-tooling/`
- `Pomegranate/src-tauri/resources/process_graph.db`
- `mechanical-knowledge-graph-service/`：保留为可选开发服务与原始服务资料，不是普通运行必需链路。

本机运行数据，不应提交仓库：

- `.runtime-data-final/`
- `.runtime-data-*`
- `Pomegranate/node_modules/`
- `Pomegranate/dist/`
- `Pomegranate/src-tauri/target/`
- `ppt-master/projects/`
- `work-test/`
- `*.log`
- `__pycache__/`

其中 `.runtime-data-final/secure-credentials/` 和 `.runtime-data-final/dev-app.db` 是本机当前运行数据，上传前不应提交；如果删除，本机配置、凭据引用和测试数据会丢失。

## 4. 启动方式

进入主程序目录：

```powershell
cd D:\ag\firstwork\Pomegranate
```

使用当前本机独立数据目录启动：

```powershell
$env:KB_DATA_DIR="D:\ag\firstwork\.runtime-data-final"
pnpm tauri:dev
```

如果换了项目目录名称，需要同步更新：

- `KB_DATA_DIR`
- `C:\Users\Yoj\AppData\Roaming\edu.bit.inb-dev\data_dir.txt`
- PPT 页面中的 `ppt-master` 根目录
- PPT 页面中的 Python 路径

PPT 常用路径：

```text
D:\ag\firstwork\ppt-master
D:\ag\firstwork\ppt-master\.venv\Scripts\python.exe
```

## 5. 上传仓库前建议清理

可以直接删除的本地生成内容：

- `Pomegranate/src-tauri/target/`
- `Pomegranate/node_modules/`
- `Pomegranate/dist/`
- `.runtime-data-phase2*`
- `.runtime-data-phase3*`
- `.runtime-data-phase4-final/`
- `.runtime-data-validation/`
- `.runtime-data-workflow-v1/`
- `.runtime-data-learning-smoke/`
- `.runtime-data/`
- `.runtime-data-final/ppt-native-quality-ab/`
- `.runtime-data-final/ppt-native-quality-strict-ab/`
- `.runtime-data-final/ppt-real-smoke/`
- `ppt-master/projects/`
- `work-test/`
- `files.v21_`
- `ppt-master/examples/ppt169_image_text_showcase/backup/`
- `__pycache__/`

不要直接删除：

- `.runtime-data-final/dev-app.db`
- `.runtime-data-final/secure-credentials/`
- `.runtime-data-final/plugins/`
- `.runtime-data-final/marketplace/`

这些不应提交，但本机继续验证时可能需要。

建议补充 `.gitignore` 覆盖：

```gitignore
.runtime-data*/
ppt-master/projects/
work-test/
files.v21_
ppt-master/examples/**/backup/
```

## 6. 验证命令

前端类型检查：

```powershell
cd D:\ag\firstwork\Pomegranate
pnpm exec tsc --noEmit
```

前端构建：

```powershell
pnpm build
```

Rust 检查：

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Rust 测试：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

PPT 相关 Node 测试：

```powershell
node --experimental-strip-types src/lib/pptContextBudget.test.ts
node --experimental-strip-types src/lib/pptMaterialChunking.test.ts
node --experimental-strip-types src/lib/pptChunkUnderstandingPrompt.test.ts
node --experimental-strip-types src/lib/pptChunkUnderstandingWorkflow.test.ts
node --experimental-strip-types src/lib/pptUnderstandingRuntime.test.ts
node --experimental-strip-types src/lib/pptUnderstandingExport.test.ts
node --experimental-strip-types src/lib/pptUnderstandingFormatting.test.ts
node --experimental-strip-types src/lib/pptUnderstandingPrompt.test.ts
node --experimental-strip-types src/lib/pptUnderstandingUi.test.ts
node --experimental-strip-types src/lib/pptUnderstandingApi.test.ts
```

## 7. 最近一次已知验证结果

最近完成的 Native Quality 严格 A/B 验收结果：

- 真实 AI 请求总数：1 次，仅用于生成唯一 `base_slide_plan`。
- A/B 后续均为离线处理、渲染和导出。
- A/B 均导出 5 页 PPTX。
- `pnpm exec tsc --noEmit` 通过。
- PPT Node 脚本测试通过。
- `pnpm build` 通过。
- `cargo fmt --check` 通过。
- `cargo check` 通过。
- `cargo test` 通过：`311 passed, 0 failed, 2 ignored`。

注意：这些结果是当时本机环境下的真实结果，上传前建议清理后重新执行一轮。

## 8. 已知风险和后续任务

优先级较高：

- 上传仓库前清理运行数据和生成产物。
- 确认 `.gitignore` 覆盖所有 `.runtime-data-*`、PPT 输出、测试目录和日志。
- 复查是否有真实密钥进入代码、日志或运行数据。
- 开发者中心、AI 市场、插件交易相关能力仍有部分本地模拟，不能描述成真实商业支付闭环。
- 星辰 Workflow 调用要根据每个工作流开始节点字段配置动态 parameters，不能强制所有工作流使用 `AGENT_USER_INPUT`。

后续可做：

- 第二份素材复验 Native Quality，再决定是否默认开启。
- 完善 PDF 原文查看和图片显示。
- 完善开发者中心、审核中心和市场商品交互。
- 继续验证 AI 助研完整业务闭环。
- 将课程知识图谱与 AI 助学或助研做受控联动。

## 9. 最短人工验收清单

上传前建议人工打开确认：

1. 首页能打开。
2. 文档、日记、待办能打开。
3. AI 助学能进入并加载知识库/题库。
4. PPT 生成页能检测 `ppt-master` 环境。
5. AI 资源中心能显示当前数据目录、凭据、智能体和会话。
6. 星辰 Workflow 使用测试凭据时能按字段 schema 构造 parameters。
7. 课程知识图谱能打开章节、搜索和详情。
8. AI 助研入口能打开。
9. 应用市场、开发者中心、审核中心能打开但不要声称真实支付可用。
10. 侧边栏在低窗口高度下可以滚动访问全部入口。

