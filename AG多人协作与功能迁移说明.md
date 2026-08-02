# AG 多人协作与功能迁移执行手册

生成时间：2026-07-31
适用项目：`D:\ag`
适用对象：AI 助学负责人、AI 助研负责人、PPT 助手负责人、最终汇总负责人
文档目的：让同伴在拿到当前项目压缩包后，可以按统一规则完成各自模块的增量完善、测试、提交和汇总。

> 本文是协作执行手册，不是最终验收报告。任何标注为【待确认】的内容，都需要执行者在自己的电脑上先确认后再开发。

## 0. 发包前必须确认

压缩包应包含：

- `src/`
- `src-tauri/`
- `services/`
- `plugins/`
- `dev-plugins/`
- `tools/`
- `scripts/`
- `templates/`
- `public/`
- `docs/`
- `package.json`
- `pnpm-lock.yaml`
- `pnpm-workspace.yaml`
- `tsconfig.json`
- `tsconfig.node.json`
- `vite.config.ts`
- `rust-toolchain.toml`
- `AGENTS.md`
- `CLAUDE.md`
- `INSTALL.md`
- `scripts/prepare-collaboration-package.ps1`
- `scripts/init-collaboration-git.ps1`
- 本文件：`AG多人协作与功能迁移说明.md`

压缩包不应包含：

- `node_modules/`
- `dist/`
- `src-tauri/target/`
- `services/**/dist/`
- `.env`
- `.env.local`
- 运行时数据库文件，明确随包资源库除外
- 日志文件
- 用户上传文件
- 生成的 PPT、PDF、图片、缓存、临时解析结果
- `D:\ag\pomegranate-local-test` 这类运行时目录

当前发包基线：

| 项目 | 当前值 |
|---|---|
| 主体项目目录 | `D:\ag` |
| 当前分支 | `feature/companion-summary` |
| Git 状态 | 当前目录可执行 Git 命令，但存在大量未跟踪文件 |
| 当前 HEAD | 当前分支暂未形成可读取提交，执行者需在自己电脑上重新确认 |
| 历史/来源完整版本 | `D:\ag\汇总3\firstwork\Pomegranate` |
| 用户曾指定重点路径 | `D:\ag\pome-combined-integration\7.9 第一周\Zhuhai\Pomegranate\Pomegranate`，当前未找到 |

发给同伴前，主负责人最好先做一次：

```powershell
git status -s
git branch --show-current
git rev-parse --show-toplevel
```

如果可以，请先由主负责人提交一个干净基线，再发包。若暂时不能提交，必须告诉同伴：压缩包中的未跟踪文件也属于当前交付基线，不允许随意删除。

如果压缩包不包含 `.git/`，同伴解压后必须先执行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\init-collaboration-git.ps1
```

该脚本会创建统一协作基线提交。所有同伴必须基于同一份压缩包初始化，之后再建立自己的功能分支。

## 1. 一句话原则

每个人只能在自己负责模块内增量完善功能，不能整目录覆盖主体项目，不能重写账号系统，不能提交用户数据，不能靠猜测合并公共文件。

## 2. 角色分工

| 角色 | 负责模块 | 主要目标 |
|---|---|---|
| A | AI 助学 | 学习计划、学情诊断、知识库问答、出题测试、学习项目与资料联动 |
| B | AI 助研 | 论文追踪、论文导入解析、多论文对比、研究方向分析、研究知识图谱 |
| C | PPT 助手 | PPT 生成、大纲、模板、排版、图片、导入导出、教学课件辅助 |
| D | 汇总负责人 | 汇总分支、处理冲突、统一测试、保护账号与打包配置 |

推荐分支：

```text
feature/learning-姓名-日期
feature/research-姓名-日期
feature/ppt-姓名-日期
integration/三模块汇总-日期
```

禁止三个人直接在同一个分支上开发。禁止直接在 `main`、`master` 或主体集成分支上改。

## 3. 项目架构速记

本项目是 Tauri 2.x 桌面应用：

```text
React 前端 src/
  -> 通过 invoke 调用
Rust Tauri 后端 src-tauri/src/
  -> Commands 层
  -> Services 层
  -> Database 层
SQLite / 本地账号目录 / Account Server 云端服务
```

前端技术栈：

- React 19
- TypeScript 5.8
- Ant Design 5
- Lucide React
- TailwindCSS 4
- Zustand
- React Router 7
- Vite 7

后端技术栈：

- Rust 2021
- Tauri 2.x
- rusqlite
- serde / serde_json
- thiserror
- chrono

新增 Tauri 功能时必须保持：

```text
models -> database -> services -> commands -> lib.rs 注册 -> src/types -> src/lib/api -> 页面调用
```

## 4. 关键目录

| 类型 | 路径 | 说明 |
|---|---|---|
| 前端源码 | `src/` | React UI |
| 页面 | `src/pages/` | 各模块页面 |
| 布局 | `src/components/layout/` | 全局导航和布局，高冲突 |
| API 封装 | `src/lib/api.ts`、`src/lib/api/index.ts` | 前端统一调用入口 |
| 状态 | `src/store/` | Zustand |
| Tauri 后端 | `src-tauri/src/` | Rust Core |
| Commands | `src-tauri/src/commands/` | IPC 入口 |
| Services | `src-tauri/src/services/` | 业务逻辑 |
| Database | `src-tauri/src/database/` | SQLite 访问与迁移 |
| Tauri 配置 | `src-tauri/tauri.conf.json` | 打包、资源、权限 |
| Capabilities | `src-tauri/capabilities/` | Tauri 2 权限 |
| Account Server | `services/account-server/` | 云端账号、文档、学习项目等 |
| 插件 | `plugins/`、`dev-plugins/` | 插件运行时和开发包 |
| 工具 | `tools/` | 题库、知识图谱等工具 |
| 文档 | `docs/` | 项目说明和架构文档 |

## 5. 账号系统保护区

业务模块可以读取当前登录状态、调用既有账号 API，但不能复制、替换、绕过或重写账号系统。

禁止普通功能开发人员修改：

| 文件或目录 | 原因 |
|---|---|
| `src-tauri/src/account.rs` | 桌面登录、系统凭据、deep link 回调、session 恢复、退出登录 |
| `src-tauri/src/account_network.rs` | 云端服务地址、部署 profile 校验 |
| `src-tauri/src/services/data_dir.rs` | 本地账号数据目录隔离 |
| `src-tauri/src/commands/data_dir.rs` | 数据目录状态暴露 |
| `src/components/layout/AccountStatusButton.tsx` | 登录状态 UI、切换账号自动重启 |
| `src/store/account.ts` | 前端当前用户状态、账号资源清理 |
| `services/account-server/src/auth.ts` | OIDC 登录、desktop ticket、session API |
| `services/account-server/src/authentication.ts` | Bearer token 读取和 session 校验 |
| `services/account-server/src/sessions.ts` | session 创建、hash、撤销 |
| `services/account-server/src/platform-users.ts` | Casdoor 身份映射 |
| `services/account-server/src/oidc.ts` | OIDC Discovery、ID Token 验签 |
| `services/account-server/src/config.ts` | 账号服务、安全配置、环境变量校验 |
| `services/account-server/migrations/001-004*.sql` | 用户、session、基础文件表 |
| `scripts/account-test/` | 账号 TEST 链路 |

可以正常使用但不能改协议：

- `accountApi.beginLogin()`
- `accountApi.restoreSession()`
- `accountApi.logout()`
- `useAccountStore((s) => s.currentUser)`
- Account Server 已有文档、文件、学习项目接口

绝对禁止：

- 在助学、助研、PPT 页面内重新做登录系统。
- 前端保存原始 session token。
- 前端把 `owner_user_id`、`student_id`、`teacher_id` 当可信参数传给服务端。
- 把 Casdoor Client Secret、数据库密码、session token、AI key 写进前端代码。
- 修改 `pomegranate://auth/callback` 协议。

## 6. 高冲突公共文件

这些文件可以改，但必须小心；每次修改都要写在迁移报告里。

| 文件 | 规则 |
|---|---|
| `src/Router.tsx` | 只新增自己的 lazy import 和 route，不重排无关路由 |
| `src/components/layout/ActivityBar.tsx` | 只新增自己的入口，不改他人入口 |
| `src/components/layout/AppLayout.tsx` | 尽量不改，必须改时单独提交 |
| `src/components/layout/Sidebar.tsx` | 不改旧逻辑，只加必要入口 |
| `src/lib/api.ts`、`src/lib/api/index.ts` | 新增模块 API 命名空间，不改账号 API |
| `src/store/account.ts` | 业务模块只读取，不修改 |
| `src-tauri/src/commands/mod.rs` | 只新增自己的 mod |
| `src-tauri/src/lib.rs` | 只新增 command 注册，不改启动和账号逻辑 |
| `src-tauri/src/database/schema.rs` | 新表独立命名，写兼容迁移 |
| `src-tauri/tauri.conf.json` | 只补资源或权限，不改 identifier 和 deep link |
| `package.json` | 新依赖先记录用途 |
| `pnpm-lock.yaml` | 汇总时统一处理更稳 |
| `src-tauri/Cargo.toml` | 新 crate 先记录用途 |
| `services/account-server/migrations` | migration 编号必须避免冲突 |

## 7. 云端与本地数据边界

| 数据 | 默认位置 | 规则 |
|---|---|---|
| 平台用户、session hash | 云端 Account Server | 只能由账号系统维护 |
| 原始 session token | 本地系统凭据 | 业务模块不可读取 |
| 当前账号本地目录 | app data 下 `accounts/<账号哈希>` | 本地缓存必须按账号隔离 |
| 用户上传文件元数据 | 云端 | 必须有服务端 owner 校验 |
| 用户上传文件内容 | `USER_FILES_ROOT` | 不进源码、不进安装包 |
| 学习项目、学习进度 | 云端优先 | 需要跨设备恢复 |
| 学习解析缓存 | 本地账号目录 | 不跨账号共用 |
| 论文 PDF 原文 | 本地优先 | 用户明确同意才上传 |
| 论文元数据 | 本地或云端 | 可同步标题、作者、DOI、摘要、标签 |
| 论文向量索引 | 本地账号目录 | 默认不同步 |
| PPT 模板资源 | `src-tauri/resources/ppt-master` | 可随包 |
| PPT 导出文件 | 用户选择目录或本地项目目录 | 不进源码 |
| PPT 作品元数据 | 云端可选 | 不自动上传原始文件 |

删除规则：

- 删除本地缓存和删除云端资料必须区分。
- 云端记录指向的文件内容不能丢。
- 多设备只能恢复已同步数据，不能假装恢复用户未上传的本地 PDF、PPT、缓存。

## 8. 三个模块的完成定义

### 8.1 AI 助学完成定义

最低可交付：

- 助学页面能正常打开，无白屏、无明显控制台错误。
- 学习项目列表可创建、查看、编辑、删除。
- 学习计划可生成或维护，并能保存到当前账号数据范围。
- 学情诊断能基于资料、题目或用户输入给出结果。
- 知识库问答能选择资料或项目上下文。
- 出题测试能生成题目、提交答案、显示结果。
- 学习进度、诊断结果、测试结果不跨账号串号。
- 上传资料必须走主体文档/文件接口或当前账号本地目录。
- 切换账号后，看不到上一个账号的学习项目和缓存。

建议增强：

- 学习资料与项目绑定。
- 错题、薄弱知识点、推荐学习路径闭环。
- 知识点 xlsx 或默认模板作为随包资源。
- 与班级/教师数据预留接口，但不绕过 membership 权限。

不得验收：

- 页面只是静态样子，没有真实保存或调用。
- 前端伪造 `owner_user_id` 来取数据。
- 上传文件写入源码目录。
- 切换账号后仍显示上一个账号数据。

### 8.2 AI 助研完成定义

最低可交付：

- 助研页面和研究库页面能正常打开。
- 可以导入或登记论文元数据。
- 论文 PDF 原文默认留在本地，不自动上传。
- 能保存标题、作者、年份、DOI、摘要、标签、阅读状态。
- 能进行单篇分析、多篇对比或研究方向摘要中的至少一条完整流程。
- 研究项目、论文库、解析缓存按当前账号隔离。
- 不泄露本地绝对路径给云端或其他账号。
- 助学知识库和助研知识库不互相污染。

建议增强：

- DOI / BibTeX / RIS 元数据导入。
- 多论文对比矩阵。
- 研究方向聚类。
- 研究知识图谱。
- 引文追踪和阅读任务管理。

不得验收：

- 自动上传本地论文 PDF。
- 将论文本地路径写入云端可见字段。
- 复制一套重复的文档库底层逻辑，导致和助学资料分裂。
- 不登录也能看到其他账号研究数据。

### 8.3 PPT 助手完成定义

最低可交付：

- PPT 页面能正常打开。
- 可以输入主题或大纲。
- 可以选择模板或版式。
- 可以生成可预览的页面结构。
- 可以导出 PPTX 或至少生成明确的本地项目输出。
- 模板、规则、脚本等随包资源放在 `src-tauri/resources/ppt-master`。
- 导出 PPTX、临时图片、中间 SVG 不写入源码目录。
- 修改 `tauri.conf.json` 时只补必要资源，不改 identifier、deep link、账号服务地址。

建议增强：

- 教学课件模板。
- 图片生成或图片选择。
- 版式检查。
- 页面重新排版。
- 导入已有 PPT 大纲。
- 导出前预览和错误提示。

不得验收：

- 只做静态页面，没有真实生成流程。
- 导出文件混入项目源码。
- 随意改 Tauri 打包标识。
- 自动上传用户 PPT 源文件。

## 9. 每位同伴的标准执行流程

### 阶段 1：环境和基线确认

先执行：

```powershell
git status -s
git branch --show-current
git rev-parse --show-toplevel
git rev-parse HEAD
corepack pnpm --version
rustc --version
cargo --version
```

必须记录：

- 主体项目路径。
- 来源项目路径。
- 自己负责模块。
- 当前分支。
- 当前 HEAD。
- 工作区是否干净。
- 来源项目是否干净。

如果 Git 不可用、当前目录不是仓库、或工作区已有陌生改动，先报告主负责人，不要自行初始化、stash、reset、clean。

### 阶段 2：只读差异分析

先比较来源项目和主体项目：

- 新增文件。
- 修改文件。
- 同名文件。
- 依赖差异。
- API 差异。
- Tauri command 差异。
- 数据库差异。
- 路由差异。
- 状态管理差异。
- 本地存储差异。
- 云端存储差异。
- 是否触碰账号系统。

不允许一开始直接复制文件。

### 阶段 3：输出迁移计划

计划必须按功能点拆分：

| 类别 | 处理方式 |
|---|---|
| 模块内新增文件 | 可以直接新增 |
| 模块内同名文件 | 逐段合并 |
| 公共文件 | 最小修改，单独说明 |
| 账号系统 | 原则上放弃来源实现，复用主体 |
| 文件/文档/上传 | 复用主体接口 |
| 数据库 | 新 migration，说明回滚和账号隔离 |
| 依赖 | 说明用途，避免重复库 |
| 运行时数据 | 不迁移 |

### 阶段 4：建立分支

```powershell
git switch -c feature/模块-姓名-日期
```

如果 `git switch` 不可用，可用：

```powershell
git checkout -b feature/模块-姓名-日期
```

禁止切到别人的分支继续改。

### 阶段 5：增量迁移

规则：

- 一次只迁一个功能点。
- 优先新增模块内部文件。
- 先适配现有 API，再考虑新增 API。
- Rust 后端保持 Command -> Service -> Database。
- 前端不裸写 `invoke()`，统一进 API 封装。
- TypeScript 不用 `any` 糊过去。
- Rust 不用 `unwrap()` 处理可能失败的操作。
- 中文注释必须 UTF-8 可读。
- 不提交运行时文件。

### 阶段 6：自测

最低命令：

```powershell
corepack pnpm build
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1
```

账号隔离建议测试：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1 -CargoArgs "test --manifest-path src-tauri\Cargo.toml account::tests --lib"
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1 -CargoArgs "test --manifest-path src-tauri\Cargo.toml services::data_dir --lib"
corepack pnpm --dir services\account-server test
```

人工测试必须覆盖：

- 应用能启动。
- 登录正常。
- 退出登录正常。
- 当前用户信息正常。
- 自己模块主流程正常。
- 其他两个模块能打开。
- 切换账号后不串号。
- 上传、导入、导出路径符合本地/云端边界。
- 控制台没有明显错误。

### 阶段 7：提交

只添加自己改过的文件：

```powershell
git add <具体文件1> <具体文件2>
git diff --cached --name-only
git commit -m "feat(助学): 接入学习项目诊断结果页面"
```

禁止：

- `git add -A`
- `git add .`
- `git stash`
- `git reset --hard`
- `git clean -fd`
- 删除自己不认识的文件

提交建议：

```text
feat(助学): 接入学习项目诊断结果页面
feat(助研): 新增论文对比分析结果视图
feat(PPT): 增加教学课件模板选择面板
fix(助学): 修复切换账号后学习资料缓存未清理
test(账号): 增加跨账号学习项目不可见测试
chore(PPT): 将 ppt-master 新模板加入打包资源
```

### 阶段 8：迁移报告

每人最终必须生成：

```text
docs/助学_迁移报告.md
docs/助研_迁移报告.md
docs/PPT_迁移报告.md
```

报告必须包含：

- 负责人。
- 模块。
- 来源目录。
- 目标目录。
- 分支名称。
- 起始 HEAD。
- 最终 HEAD。
- 新增功能。
- 修改文件。
- 新增文件。
- 删除文件。
- 修改的公共文件。
- 是否修改账号系统。
- 是否修改数据库。
- 是否修改云端接口。
- 是否新增本地存储。
- 是否新增云端存储。
- 新增依赖。
- 新增环境变量。
- 测试命令和结果。
- 人工验收记录。
- 已知问题。
- 未迁移内容。
- 汇总注意事项。

## 10. 模块负责人具体指令

### 10.1 AI 助学负责人

允许修改：

- `src/pages/learning-assistant/`
- `src/lib/learning/`
- `src-tauri/src/services/learning_assistant.rs`
- `src-tauri/src/services/learning_kb.rs`
- `src-tauri/src/services/learning_progress.rs`
- `src-tauri/src/services/learning_quiz.rs`
- `src-tauri/src/services/learning_resources.rs`
- `src-tauri/src/services/local_learning_plan.rs`
- `src-tauri/src/commands/learning_*`
- 必要时新增 `services/account-server/src/learning-*`
- 必要时新增独立编号 migration

谨慎修改：

- `src/Router.tsx`
- `src/components/layout/ActivityBar.tsx`
- `src/lib/api.ts`
- `src/lib/api/index.ts`
- `services/account-server/src/documents.ts`
- `services/account-server/src/document-library.ts`
- `services/account-server/migrations/`

迁移重点：

- 学习计划、学情诊断、测试结果、学习项目应走云端账号体系。
- 知识点 xlsx、默认模板、离线资源可放 `src-tauri/resources/learning-assistant/`。
- 用户上传资料走 Account Server 文件/文档 API。
- 本地缓存必须落入当前账号专属目录。

### 10.2 AI 助研负责人

允许修改：

- `src/pages/research-assistant/`
- `src/pages/research-library/`
- `src/lib/researchKnowledgeBase.ts`
- `src/lib/researchKnowledgeBaseCore.ts`
- `src-tauri/src/services/research_analysis.rs`
- `src-tauri/src/services/research_papers.rs`
- `src-tauri/src/services/research_recommendation.rs`
- `src-tauri/src/commands/research_*`

谨慎修改：

- `src-tauri/src/services/source_file.rs`
- `src-tauri/src/commands/source_file.rs`
- `src-tauri/src/database/document_sources.rs`
- `src/lib/api.ts`
- `src/lib/api/index.ts`
- `src/Router.tsx`
- 文档库、知识库共用逻辑

迁移重点：

- 论文 PDF 原文默认本地。
- 论文元数据可同步。
- 解析缓存、向量索引、中间输出默认本地且按账号隔离。
- 优先复用主体文档/资料接口，不复制重复底层存储。

### 10.3 PPT 助手负责人

允许修改：

- `src/pages/ppt-generation/`
- `src/lib/ppt*`
- `src/store/pptGenerationDraft.ts`
- `src-tauri/src/services/ppt_master*.rs`
- `src-tauri/src/commands/ppt_master.rs`
- `src-tauri/resources/ppt-master/`

谨慎修改：

- `src/Router.tsx`
- `src/components/layout/ActivityBar.tsx`
- `src-tauri/tauri.conf.json`
- `package.json`
- `src-tauri/Cargo.toml`

迁移重点：

- PPT 生成、模板、视觉检查、导入导出本地优先。
- 模板、规则、脚本可随包。
- 导出 PPTX、临时图片、中间 SVG 默认留本地。
- 如需云端同步，只同步用户明确选择的作品或元数据。

## 11. 汇总负责人流程

汇总顺序建议：

1. 备份主体项目。
2. 确认主体分支、HEAD、工作区状态。
3. 收集三个人分支、提交、迁移报告。
4. 先看每个人 `git diff --name-only`。
5. 检查是否改了账号系统保护区。
6. 检查是否提交了用户数据、缓存、构建产物。
7. 按“助学 -> 助研 -> PPT”逐个 cherry-pick 或 merge。
8. 每合并一个模块立即运行构建和人工冒烟测试。
9. 公共文件冲突手工处理。
10. 账号系统文件默认保留主体版本，分支改动单独审查。
11. migration 编号冲突时重新编号并检查依赖顺序。
12. 统一安装依赖并生成锁文件。
13. 做全功能回归。
14. 最后生成正式集成提交。

推荐优先 cherry-pick：

- 同伴提交清晰。
- 只要部分功能。
- 分支里有实验提交。

可以 merge：

- 分支干净。
- 整个模块都要。
- 没有账号系统高风险改动。

不要直接复制最终文件夹覆盖，因为会丢提交历史，也看不出谁改了账号、路由、依赖、数据库。

## 12. 汇总验收清单

全局验收：

- 应用可启动。
- 登录可用。
- 退出登录可用。
- 当前用户信息显示正确。
- 切换账号后本地数据隔离。
- 三个模块页面均可打开。
- 构建通过。
- 没有提交运行时文件。
- 没有前端密钥。
- 没有修改 deep link、identifier、账号服务协议。

助学验收：

- 学习项目 CRUD 可用。
- 学习计划可保存和恢复。
- 诊断结果可保存。
- 测试结果不串账号。
- 上传资料归属当前账号。

助研验收：

- 论文元数据导入或创建可用。
- PDF 原文默认不上传。
- 论文分析或对比流程可跑通。
- 本地路径不泄露。
- 研究库不串账号。

PPT 验收：

- 大纲或主题输入可用。
- 模板选择可用。
- 生成预览可用。
- 导出或本地输出可用。
- 输出文件不进源码。
- 打包资源包含必要模板。

## 13. 给同伴的 Codex 提示词模板

### 13.1 助学

```text
我是【姓名】，负责 AI 助学模块。
主体项目路径是：【主体项目路径】。
来源项目路径是：【来源项目路径】。
目标分支名称是：【feature/learning-姓名-日期】。
允许迁移的功能是：【列出具体功能】。

请先只读分析，不要复制文件、不要修改代码。
必须先阅读：
- AG多人协作与功能迁移说明.md
- AGENTS.md
- docs/account-classroom-isolation.md
- docs/summary3-cloud-local-archive.md

重点检查：
- src/pages/learning-assistant
- src/lib/learning
- src-tauri/src/services/learning_*
- src-tauri/src/commands/learning_*
- services/account-server/src/learning-*
- services/account-server/migrations

要求：
1. 只做增量迁移，禁止整目录覆盖。
2. 复用主体账号、文件、文档、数据目录隔离能力。
3. 学习项目、计划、诊断、测试结果需要按账号隔离。
4. 用户上传资料走主体文件/文档 API。
5. 不允许修改账号登录、session、deep link、OIDC。
6. 先输出只读差异分析和迁移计划，经确认后再改代码。
7. 完成后运行构建和账号隔离测试。
8. 在 docs/ 生成 助学_迁移报告.md。
```

### 13.2 助研

```text
我是【姓名】，负责 AI 助研模块。
主体项目路径是：【主体项目路径】。
来源项目路径是：【来源项目路径】。
目标分支名称是：【feature/research-姓名-日期】。
允许迁移的功能是：【列出具体功能】。

请先只读分析，不要复制文件、不要修改代码。
必须先阅读：
- AG多人协作与功能迁移说明.md
- AGENTS.md
- docs/account-classroom-isolation.md
- docs/summary3-cloud-local-archive.md

重点检查：
- src/pages/research-assistant
- src/pages/research-library
- src/lib/researchKnowledgeBase*
- src-tauri/src/services/research_*
- src-tauri/src/commands/research_*
- src-tauri/src/services/source_file.rs
- src-tauri/src/database/document_sources.rs

要求：
1. 论文 PDF 原文默认本地，用户明确同意后才上传。
2. 论文元数据可同步，缓存和向量索引按账号本地隔离。
3. 不泄露本地绝对路径。
4. 优先复用主体文档/资料接口。
5. 不允许修改账号登录、session、deep link、OIDC。
6. 先输出只读差异分析和迁移计划，经确认后再改代码。
7. 完成后运行构建和账号隔离测试。
8. 在 docs/ 生成 助研_迁移报告.md。
```

### 13.3 PPT

```text
我是【姓名】，负责 PPT 助手模块。
主体项目路径是：【主体项目路径】。
来源项目路径是：【来源项目路径】。
目标分支名称是：【feature/ppt-姓名-日期】。
允许迁移的功能是：【列出具体功能】。

请先只读分析，不要复制文件、不要修改代码。
必须先阅读：
- AG多人协作与功能迁移说明.md
- AGENTS.md
- docs/account-classroom-isolation.md
- docs/summary3-cloud-local-archive.md

重点检查：
- src/pages/ppt-generation
- src/lib/ppt*
- src/store/pptGenerationDraft.ts
- src-tauri/src/services/ppt_master*.rs
- src-tauri/src/commands/ppt_master.rs
- src-tauri/resources/ppt-master
- src-tauri/tauri.conf.json 的 bundle.resources

要求：
1. PPT 生成、模板、视觉检查、导入导出本地优先。
2. 模板资源放入 src-tauri/resources/ppt-master。
3. 导出 PPTX、临时图片、中间 SVG 不进入源码。
4. 云端同步必须用户明确选择。
5. 不允许修改账号登录、session、deep link、OIDC。
6. 先输出只读差异分析和迁移计划，经确认后再改代码。
7. 完成后运行构建和打包资源检查。
8. 在 docs/ 生成 PPT_迁移报告.md。
```

## 14. 失败时如何处理

如果构建失败：

- 先记录失败命令和错误摘要。
- 判断是自己模块、公共文件、依赖、Rust 编译还是账号服务问题。
- 只修自己引入的问题。
- 不要为了通过构建删除其他模块代码。

如果出现账号问题：

- 立即停止改账号文件。
- 记录复现步骤。
- 报告是否改过保护区。
- 默认恢复为主体账号实现，由汇总负责人审查。

如果出现 migration 冲突：

- 不要手工拼接旧编号。
- 报告新增表、字段、索引、约束。
- 由汇总负责人统一编号。

如果发现来源项目功能和主体架构冲突：

- 保留功能目标。
- 放弃来源底层实现。
- 改为接入主体账号、文档、文件、本地数据目录和 API 体系。

## 15. 最终交付物

每位同伴最终交付：

```text
1. Git 分支名
2. 提交列表
3. 模块迁移报告
4. 测试命令和结果
5. 人工验收截图或步骤
6. 新增依赖清单
7. 新增环境变量清单
8. 新增本地数据目录清单
9. 新增云端接口清单
10. 数据库变更说明
11. 未完成问题说明
```

禁止只交一个修改后的完整文件夹。最终负责人需要的是可追踪的分支、提交、差异和报告。
