# 同伴 Codex 功能移植协作指令

这份文件发给负责不同功能的同伴。目标是：让每个人把自己电脑上已经做好的更完善功能，移植进当前 `AG` 主体工程，同时不破坏账号系统稳定性、云端/本地边界、后续打包和最终汇总。

## 给同伴的总要求

你拿到的是主工程副本，不是一次性实验目录。迁移时必须保留现有功能性质，不允许为了合并方便删减功能、改弱功能、绕过账号系统或把运行时数据塞进源码目录。

账号系统部分已经作为稳定底座，除非任务明确要求修复账号系统，否则不要重写、替换或绕开下面这些文件和规则：

- 桌面登录、凭据分槽、session 恢复：`src-tauri/src/account.rs`
- 当前账号本地数据目录隔离：`src-tauri/src/services/data_dir.rs`
- 前端账号状态与切换账号自动重启：`src/components/layout/AccountStatusButton.tsx`
- 前端账号 API 类型：`src/lib/api.ts`、`src/lib/api/index.ts`
- 云端账号、session、文件、文档、学习项目：`services/account-server`
- 账号/班级/插件安全规则：`docs/account-classroom-isolation.md`
- 云端/本地归档边界：`docs/summary3-cloud-local-archive.md`

## 可以直接复制给 Codex 的任务提示词

```text
我正在完善 Pomegranate 主工程。请先阅读 docs/collaborator-codex-migration-brief.md、docs/account-classroom-isolation.md、docs/summary3-cloud-local-archive.md。

我的任务范围是：【在这里写：助学 / 助研 / PPT / 其他具体功能】。
我的完善版功能代码来源在：【在这里写你电脑上的来源文件夹路径】。
目标主工程在：【在这里写当前 AG 主工程路径】。

请把来源中的完整功能移植进主工程，不允许功能缩水，不允许改变功能性质。技术路线可以不同，但必须达到同等效果。

必须遵守：
1. 不重写或绕过现有账号登录、session、凭据保存、本地账号数据隔离。
2. 需要跨设备、账号同步、班级师生联动、审计、共享、插件市场的内容归云端服务。
3. 只属于本机能力、离线缓存、设备处理、随包资源、用户私有临时产物的内容保留本地。
4. 云端数据只能用服务端 session 得到当前 platformUserId，不接受前端传 owner_user_id/student_id/teacher_id 作为授权依据。
5. 本地缓存或本地数据库若保存账号相关内容，必须落入当前账号专属数据目录，不能多账号共用。
6. 前端涉及账号云端数据时优先通过现有 Tauri account bridge，不要直接把 token 暴露给 Web 层。
7. 不提交 .env、真实密钥、PostgreSQL 数据目录、user-files、node_modules、target、dist、生成的 PPT 项目结果。

完成后请输出：
- 改了哪些文件。
- 从来源迁移了哪些功能，是否完整保留。
- 哪些属于云端，哪些属于本地。
- 是否碰了账号系统；如果碰了，说明原因和对应测试。
- 运行过的验证命令和结果。
- 仍需人工验收的场景。
```

## 功能分工落点

| 负责方向 | 前端主落点 | 本地 Rust 主落点 | 云端主落点 | 边界要求 |
|---|---|---|---|---|
| 助学 | `src/pages/learning-assistant`、`src/lib/learning` | `src-tauri/src/services/learning_*`、`local_learning_plan.rs`、`document_tree.rs` | `services/account-server` 的学习项目、文档、文件 API；后续班级学习事件 | 知识点资源可本地随包；学习项目、上传资料、跨设备同步和班级联动归云端 |
| 助研 | `src/pages/research-assistant`、`src/pages/research-library` | `src-tauri/src/services/research_*`、`planning.rs`、`source_file.rs` | 需要账号同步的论文库、项目库、协作记录归云端 | 本地解析和临时缓存留本地；云端记录必须按当前账号或授权班级隔离 |
| PPT | `src/pages/ppt-generation` | `src-tauri/src/services/ppt_master*.rs`、`src-tauri/resources/ppt-master` | 只把用户选择同步的成品、模板市场或项目元数据上云 | PPT 生成引擎和随包资源保留本地；生成项目输出不进源码和安装包资源 |
| 题库/练习 | 后续新增页面或接入助学页面 | 可有本地导入/审核工具 | `services/question-bank` 或 `services/account-server` 代理模块 | 学生身份由服务端注入；学生取题不能返回答案；教师视图必须校验班级成员关系 |
| 插件 | `src/pages/plugins`、`src/pages/developer-center`、`src/services/pluginManager.ts` | `src-tauri/src/services/plugins.rs`、`plugin_*`、`commands/plugin_proxy.rs` | `services/plugin-registry` | 插件不能拿真实 session token、AI key、Casdoor token；所有账号数据经代理授权 |

## 云端和本地判断规则

归云端：

- 账号身份、session、账号编号。
- 用户上传文件、文档库、学习项目、研究项目、跨设备同步数据。
- 班级、成员、教师/学生角色、邀请、作业、消息、学习事件。
- 题库正式数据、学生答题、错题、推荐、教师统计。
- 插件市场、插件版本、审核、授权、组织/班级策略。

保留本地：

- Tauri 桌面壳、窗口、系统集成。
- 本地笔记、任务、标签、搜索索引和离线缓存。
- PDF、PPT、Office、图片等本机解析和生成流程。
- `src-tauri/resources/course-graph/process_graph.db`。
- `src-tauri/resources/learning-assistant/knowledge-points/*.xlsx`。
- `src-tauri/resources/ppt-master` 中必须随包的 PPT 引擎资源。
- 生成的临时文件、PPT 项目输出、导出结果、用户本机草稿。

拿不准时按这个原则判断：多个账号、多个设备、师生共享、需要权限审计的数据归云端；只依赖本机能力、可重建、可离线、体积大且不需要共享的处理过程留本地。

## 账号隔离硬性规则

迁移功能时，凡是涉及账号数据，必须满足：

1. 云端表必须有 `owner_user_id` 或 `class_id`。
2. `owner_user_id` 只能由服务端从 Bearer session 得到，不能由前端传入。
3. 查询、下载、删除、恢复、复制都要带当前账号或授权班级条件。
4. 跨账号访问统一返回 404 或等价安全错误，不泄露资源是否存在。
5. 本地缓存要跟随当前账号专属数据目录，不能写死到公共目录。
6. 登录 token、Casdoor token、AI provider key 不进 localStorage、普通 JSON、SQLite 明文字段或插件 JS。
7. 切换账号后，功能页面不能继续展示上一个账号的本地缓存或云端列表。

## 禁止做的事

- 不要把 `汇总3/`、个人完整来源目录、实验数据目录直接复制进安装包资源。
- 不要把 `.env`、数据库、用户上传文件、真实密钥、账号 token 提交或打包。
- 不要让前端直接保存或拼接 `owner_user_id`、`student_id`、`teacher_id` 来实现权限。
- 不要为了省事把所有功能都塞进本地 SQLite；需要共享和师生联动的必须走云端。
- 不要为了省事把所有功能都塞进云端；PPT/PDF/本机转换、离线缓存和随包资源应留本地。
- 不要删除现有测试来让构建通过。
- 不要改安装包名字、identifier、deep link scheme，除非主负责人明确要求。

## 推荐迁移流程

1. 先列来源功能清单：页面、服务、资源、数据库、配置、测试。
2. 给每个功能标注归属：云端、本地、插件扩展或仅开发工具。
3. 只迁当前负责人范围内的功能，不顺手大改其他模块。
4. 先接入主工程已有 API、命令、数据模型；缺接口时补最小稳定接口。
5. 云端新增表必须写 migration，并补跨账号隔离测试。
6. 本地新增缓存必须走当前账号数据目录，或明确证明它不含账号数据。
7. 保持 UI 路由和入口可被主工程导航发现，不做孤立页面。
8. 迁完后跑对应构建和测试。
9. 写迁移报告，说明功能是否完整、哪些地方技术路线不同但效果一致。

## 最低验证命令

在 Windows PowerShell 下优先运行：

```powershell
corepack pnpm build
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1 -CargoArgs "test --manifest-path src-tauri\Cargo.toml account::tests --lib"
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1 -CargoArgs "test --manifest-path src-tauri\Cargo.toml services::data_dir --lib"
corepack pnpm --dir services\account-server test
```

如果只改了某个功能模块，还要补跑该模块自己的测试。没有测试的功能，必须至少写清楚人工验收步骤。

## 交回主负责人时必须附带

每个同伴交回时，在说明里写清楚：

```text
负责人：
负责模块：
来源路径：
迁移目标路径：

已完整迁移的功能：
- 

技术路线不同但效果一致的地方：
- 

云端新增/修改：
- API：
- migration：
- 数据归属字段：
- 隔离测试：

本地新增/修改：
- 页面：
- Tauri 命令/服务：
- 资源文件：
- 本地缓存位置：

是否触碰账号系统：
- 否 / 是，原因：

已运行验证：
- 命令：
- 结果：

仍需人工验收：
- 
```

## 最终汇总原则

主负责人汇总时，只接受满足下面条件的功能：

- 功能完整，不是局部演示或空壳。
- 云端/本地边界清楚。
- 不破坏账号登录、session 恢复、退出、切换账号、本地账号数据隔离。
- 跨账号云端数据不可见。
- 可打包，且安装包不携带运行时数据和密钥。
- 改动范围清楚，助学、助研、PPT 能分开审核，也能最后合并到同一主工程。

如果某个功能必须改账号底座才能完成，先单独提出账号系统变更方案和测试，不要混在功能迁移里直接改。
