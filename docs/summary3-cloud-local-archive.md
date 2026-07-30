# 汇总3功能归档与云端/本地边界

本文档是 `汇总3/firstwork` 完整版本功能进入当前 `D:\ag` 主项目的归档总图。原则是：功能不缩水、不改变功能性质；先明确归属与后续落点，再逐项迁移实现。

## 当前结论

`汇总3/firstwork` 是同一软件的完整功能来源归档，当前不得删除、覆盖或只按局部功能搬运。后续迁移时按下面三类归属执行：

- 云端：账号、用户文件、学习项目、文档库、班级、师生消息、题库正式服务、插件市场/审核/授权。
- 本地：Tauri 桌面壳、本地笔记与任务、本地资源库、PDF/PPT 处理、课程知识图谱 SQLite、学习助手知识点、离线缓存、插件运行时。
- 插件扩展：插件源码包、声明式能力、扩展点、插件开发中心、插件审核中心、插件权限与令牌边界。

## 统一文件夹结构

```text
D:\ag
  services/
    account-server/              # 已存在：云端账号、会话、用户文件、文档库、学习项目
    classroom/                   # 预留：班级、师生关系、作业/消息/学习事件
    question-bank/               # 预留：正式题库服务或题库代理，不能让客户端直传 student_id
    plugin-registry/             # 预留：插件市场、审核、授权、版本分发
  src/
    ...                          # 桌面前端，按账号状态调用云端或本地能力
  src-tauri/
    src/                         # 本地 Rust 能力：文件、笔记、PPT、PDF、图谱、插件代理
    resources/
      course-graph/              # 已存在：process_graph.db
      learning-assistant/        # 已存在：知识点 xlsx
      ppt-master/                # 预留：需要随包带走的 PPT 引擎资源
  plugins/
    README.md                    # 插件源码包与示例插件归档入口
  archive/
    summary3/README.md           # 汇总3索引；原始完整归档仍保留在 汇总3/firstwork
  docs/
    summary3-cloud-local-archive.md
    account-classroom-isolation.md
    summary3-archive-manifest.json
```

上面 `services/classroom`、`services/question-bank`、`services/plugin-registry` 是稳定归档落点，可以先作为 `account-server` 的模块实现，后续再拆成独立服务；路径命名先固定，避免以后迁移时接口和数据模型反复改名。

## 功能归属清单

| 汇总3来源 | 功能 | 归属 | 当前/后续落点 | 保留要求 |
|---|---|---|---|---|
| `汇总3/firstwork/Pomegranate` | 主桌面应用完整版本 | 本地 + 云端客户端 | `src/`, `src-tauri/` | 逐项合并，不能用空壳替换现有功能 |
| `learning-assistant` | 学习计划、知识点、资源/题目模板 | 本地 + 云端学习项目 | 本地资源进 `src-tauri/resources/learning-assistant/`；可同步项目进 `services/account-server` | 本地可离线，云端只保存账号内学习项目和文档关联 |
| `files.v21_最终/question_bank_system` | 学生练习、答题、推荐、错题、教师接口、图片资产 | 云端正式服务，开发期可本地工具 | `services/question-bank/` 或 `services/account-server` 代理模块 | 学生接口不得返回答案；学生身份由服务端注入 |
| `question-bank-tooling` | 题库生产、清洗、审核、发布工具 | 本地/内部开发工具 | 保留在归档；后续若产品化再拆 `tools/question-bank/` | 不随桌面普通安装包发布，不作为学生运行时依赖 |
| `mechanical-knowledge-graph-service` | 原 Neo4j/FastAPI 知识图谱服务 | 归档 + 可选开发服务 | 保留原始服务；正式桌面使用 SQLite 资源 | 普通运行不依赖 Neo4j/FastAPI |
| `process_graph.db` | 课程知识图谱 SQLite | 本地随包资源 | `src-tauri/resources/course-graph/process_graph.db` | 已是本地资源，打包时必须包含 |
| `ppt-master` | PPT 生成、模板、视觉检查 | 本地引擎 + 可选插件扩展 | `src-tauri/src/services/ppt_master*.rs`，必要资源进 `src-tauri/resources/ppt-master/` | 生成能力不缩水；项目输出不进源码包 |
| 插件管线文档 | AI 应用市场、插件开发中心、审核中心、场景增强 | 插件扩展 + 云端市场 | 本地运行时：`src-tauri/src/services/plugins.rs`、`src/services/pluginManager.ts`；云端市场预留 `services/plugin-registry/` | 插件不能越权读写账号数据，不能直接拿用户密钥 |
| 文档导入/AI 资料中心 | 上传、资料库、AI 理解、星火流程 | 云端文档库 + 本地导入处理 | `services/account-server` 文档/文件 API，`src-tauri` 导入/PDF/解析服务 | 文件归账号所有，本地缓存不能串号 |

## 云端边界

云端只放需要账号、跨设备、师生共享、审计或集中分发的内容：

- 账号身份：Casdoor 登录后的平台用户、会话、账号号。
- 用户文件：上传文件、文档库、学习资料、学习项目文档关联。
- 学习项目：学习目标、理解结果、计划、进度、调整历史。
- 班级联动：班级、成员、教师/学生角色、邀请、作业、消息、学习事件。
- 题库正式数据：题目、审核状态、使用范围、学生答题、错题、推荐结果。
- 插件市场：插件包版本、审核状态、适用场景、授权范围、启用策略。

云端数据表必须用 `platform_users.id` 做内部主体，接口不得相信客户端传入的 `owner_user_id`、`student_id`、`teacher_id`。

## 本地边界

本地只放设备能力、离线能力、用户私有工作区和随包资源：

- Tauri 桌面运行、窗口、系统集成、文件导入导出。
- 本地 SQLite 笔记、任务、标签、附件、搜索索引。
- 本地数据目录和缓存，不能默认落进项目源码目录。
- `src-tauri/resources/course-graph/process_graph.db`。
- `src-tauri/resources/learning-assistant/knowledge-points/*.xlsx`。
- PDFium、PPT 引擎、文档转换、临时生成结果。
- 本地安装插件运行目录 `data_dir/plugins/<plugin-id>`。

如果同一台电脑登录多个账号，本地缓存必须按账号命名空间隔离，例如 `data_dir/accounts/<platformUserId>/...`。没有完成这个隔离前，不应开启多账号无感切换。

## 插件边界

当前已有本地插件运行时：`src-tauri/src/services/plugins.rs`、`src-tauri/src/commands/plugin_proxy.rs`、`src/services/pluginManager.ts`。后续从汇总3迁插件功能时，必须保持：

- 插件通过 manifest 声明能力和权限。
- 插件调用本地能力只走 `plugin_proxy_*`，由 Rust 校验令牌和权限。
- 插件设置按 `plugin_id` 隔离。
- 插件不能直接读取云端 session token、Casdoor token、AI provider key 或本地绝对路径。
- 插件市场、审核、授权属于云端；插件执行和本地能力代理属于桌面端。

## 打包边界

桌面安装包应包含：

- `dist/` 前端构建产物。
- Tauri 主程序。
- `src-tauri/resources/course-graph/process_graph.db`。
- `src-tauri/resources/learning-assistant/knowledge-points/*.xlsx`。
- 必需的 PDF/PPT 本地运行资源。

桌面安装包不应包含：

- `汇总3/` 原始归档。
- `node_modules/`、`target/`、`dist/` 以外的构建中间目录。
- `question-bank-tooling` 的生产/清洗数据和内部审核材料。
- `services/account-server` 的运行时数据、PostgreSQL 数据目录、密钥和 `.env`。
- `ppt-master/projects/` 生成结果。

云端部署包应包含：

- `services/account-server`。
- 后续 `services/classroom`、`services/question-bank`、`services/plugin-registry`。
- 数据库 migration。
- 对象存储或文件存储配置。

云端部署包不应包含桌面用户本地数据、桌面缓存、设备路径或源码归档包。

## 迁移顺序

1. 先补班级与账号隔离模型，再接题库学生/教师数据。
2. 题库先通过云端代理接入，服务端注入 `student_id`，再考虑迁 PostgreSQL。
3. 学习助手知识点保持本地资源，学习项目和资料关联继续走云端账号服务。
4. PPT 能力保持本地引擎，云端只保存用户选择同步的成品或项目元数据。
5. 插件先统一 manifest 与权限，再接插件市场/审核中心。

这份文档是归档入口，不代表功能删减。每次迁移功能时，以“来源路径 + 目标边界 + 验证项”三件套记录，确保完整版本能力可追踪。
