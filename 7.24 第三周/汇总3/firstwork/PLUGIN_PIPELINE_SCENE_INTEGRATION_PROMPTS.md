# Pomegranate 插件技术路线、宿主接口与各板块 Codex 任务提示词

## 1. 文档用途

本文档用于把尚未接入 `pluginPipeline` 的业务页面分派给不同开发者。每个任务都必须以当前 `D:\ag\firstwork\Pomegranate` 为基线进行最小修改，禁止复制旧版本页面或建立第二套插件流水线。

### 1.1 面向团队的一句话总结

Pomegranate 当前已经验证了一条可行路线：开发者把声明式插件源码打包成一个 `.firstwork-plugin` 压缩安装包，通过开发者中心上传，经过 Manifest v3 安全预检和管理员审核后发布到 AI 应用市场；用户获取、确认权限并安装后，插件可以提供独立功能页面，或者在已经接入 `pluginPipeline` 的业务功能中增强 AI 输入、上下文、提示词和输出。

### 1.2 “上传一个压缩包”具体指什么

这里的压缩包不是任意 `.zip`，而是由当前工具生成的 `.firstwork-plugin`：

```text
插件源码目录
→ pnpm.cmd plugin:pack
→ com.example.plugin-1.0.0.firstwork-plugin
→ pnpm.cmd plugin:verify
→ 开发者中心上传
```

安装包本质上使用受控 ZIP 容器，但必须包含 Manifest v3、声明式资源和 SHA-256 校验文件。它不能包含 JavaScript、Python、Shell、可执行程序、`.env`、私钥或真实 API 凭据。

开发者中心和市场只负责“包的上传、审核、分发和安装”。一个增强插件能否在某个业务页面产生作用，还取决于该页面是否提供了本文件第 3、4 节描述的宿主接口。

### 1.3 各板块同学如何使用本文档

1. 先阅读第 2 节，理解当前已经验证的插件类型和边界。
2. 根据第 4 节找到自己负责的功能板块和需要补齐的接口。
3. 到第 7 节以后找到对应的 Codex 任务提示词，完整复制给自己负责代码修改的 Codex。
4. 每位同学只修改自己的业务入口，不自行复制或重写 `pluginPipeline`。
5. 所有分支汇总后，由统一集成人员执行最后的跨场景回归任务。

当前事实：

- Manifest v3 已支持 `feature`、`enhancement`、`hybrid`。
- 统一前端流水线位于 `src/services/pluginPipeline.ts`。
- Rust 负责插件启用状态、场景、权限、依赖、冲突、顺序和审计解析。
- 当前 `PluginScene` 只有 `global`、`learning`、`research`、`teaching`。
- AI 助学的 `goal-understanding`、`learning-planner` 已接入，可作为参考实现。
- 正式插件只能使用声明式资源，不能执行插件 JavaScript，也不能获得通用 Tauri invoke。

## 2. 当前已经验证可行的插件方案

### 2.1 Feature 独立功能插件

已经验证的代表是“文档智能总结”：

- 使用 Manifest v3。
- `classification` 为 `feature`。
- `runtimeKind` 为受控星辰 Workflow 类型。
- `handler.kind` 为 `declarative`。
- 由 `PluginFeatureHost` 根据 `uiSchema` 生成表单。
- 插件只保存 `externalAgentId` 或用户选择的服务引用，不保存 API Key、API Secret、Token。
- 开发者上传、Manifest v3 预检、审核、市场展示、获取、权限确认和 `PluginPlatformService` 安装已通过服务层测试。
- 真实星辰调用仍取决于用户在 AI 资源中心配置的 Workflow，不能把 Mock 或未配置状态描述成真实调用成功。

适合场景：插件需要自己的表单、按钮、结果区域或独立使用入口。

### 2.2 Enhancement 增强型插件

已经验证的代表是 `com.example.learning-enhancer`：

- 使用 `classification: "enhancement"`。
- 使用 `runtimeKind: "prompt-pack"`。
- 通过 `contributes.enhancements` 声明 Hook、场景和目标功能。
- 只包含 Markdown 等声明式资源，不执行插件代码。
- 已在 AI 助学页面显示“声明式学习增强：1 个处理步骤已执行”，证明安装、启用、场景解析、权限和 Hook 执行链可工作。
- 当前插件使用 `promptEnhancer`，因此只有进入真实模型调用时才能明显影响回答；本地模板模式不会理解追加的提示词。

适合场景：不增加独立页面，而是在现有 AI 功能调用前后补充输入处理、上下文、提示词、输出整理或安全 UI 提示。

### 2.3 Hybrid 混合插件

当前平台已经自动验证：

- `classification: "hybrid"` 必须同时声明 `features` 和 `enhancements`。
- Feature 部分复用 `PluginFeatureHost`。
- Enhancement 部分复用 `pluginPipeline`。
- 上传、预检、审核、安装和贡献点解析使用同一 Manifest v3 平台。

当前边界：Hybrid 的结构校验和两类贡献解析已有自动测试，但尚缺少一个专用 Hybrid 插件完成全部桌面 GUI 人工验收。因此应标记为“平台支持、GUI 组合链待人工复验”，不能写成所有场景均已验证。

### 2.4 当前完整生命周期

```text
插件源码
→ plugin:pack 生成 .firstwork-plugin
→ plugin:verify 离线复验
→ 开发者中心上传
→ Manifest v3 预检和安全扫描
→ 提交审核
→ 审核批准并锁定包 SHA-256
→ AI 应用市场展示具体批准版本
→ 用户获取和确认权限
→ PluginPlatformService 安装
→ 插件管理启用全局/场景
→ 业务宿主调用 pluginPipeline 或 PluginFeatureHost
→ 执行审计
→ 更新、回滚、停用或卸载
```

市场负责分发，`PluginPlatformService` 负责安全安装和贡献解析，业务页面负责提供宿主接口。插件成功安装不等于它自动进入所有页面。

### 2.5 已有验证证据和不能夸大的部分

| 项目 | 当前证据 | 结论 |
| --- | --- | --- |
| 插件打包与复验 | `plugin:pack`、`plugin:verify` 已对真实示例包通过 | `.firstwork-plugin` 可以被当前工具可靠生成和读取 |
| 市场发布桥接 | 文档智能总结真实包夹具完成上传、预检、审核、市场可见、获取、权限确认和安装服务测试 | Manifest v3 可以经过本地市场供应链 |
| Enhancement 执行 | AI 助学页面显示 `com.example.learning-enhancer` 且“1 个处理步骤已执行” | AI 助学宿主流水线已经实际执行增强贡献 |
| 安全与兼容 | Rust 全量测试曾达到 330 passed、0 failed、2 ignored；v2 兼容测试保留 | 当前 v3 桥接没有替换旧 v2 链路 |
| Feature 真实 AI 调用 | 依赖用户配置的 ExternalAgent/星辰 Workflow | 自动测试未使用真实凭据，仍需人工配置验证 |
| Hybrid 完整 GUI | 有 Manifest 和解析测试 | 尚缺专用 Hybrid 包的完整桌面人工验收 |
| 其他业务板块增强 | 多数尚未调用 `pluginPipeline` | 不能因为插件成功安装就宣称已作用于所有页面 |

## 3. 业务页面接入插件系统需要准备的接口

### 3.1 能力定位接口

每次调用都需要稳定提供：

- `scene`：当前业务场景，现有值为 `global / learning / research / teaching`。
- `feature`：当前具体能力，例如 `ai-chat-message`、`document-summary`。
- `sessionId`：会话型功能使用，用于隔离流式事件和插件状态。
- `workspaceId`：文档、项目或任务型功能使用。
- `requestId`：一次调用的唯一标识，用于取消、审计和防止串台。
- `userRole`：仅提供 `student / teacher / unknown` 等业务角色，不提供账号凭据。

这些字段构成 `PluginExecutionContext`，由宿主页面提供，不能让插件自行猜测当前页面或用户身份。

### 3.2 原始输入与有效输入分离接口

业务调用至少需要区分：

```text
originalInput：用户原始输入，用于界面显示和历史保存
effectiveInput：经过 inputProcessor 后供本次业务/模型使用的输入
```

禁止把 `effectiveInput` 覆盖回用户已经保存的消息、文档或表单。这样可以避免增强插件静默篡改用户数据。

### 3.3 隐藏上下文接口

宿主需要能够把以下内容安全传给 Provider，但不显示在聊天气泡或文档中：

- `contextProvider` 提供的背景信息；
- `promptEnhancer` 提供的提示约束；
- 场景级系统说明；
- 非敏感的插件贡献点来源。

普通 OpenAI 兼容模型优先使用 system/developer message；只接受单段文本的星辰 Workflow 由 Rust 后端构造隐藏前缀。不能在 React 中把隐藏提示拼进可见用户消息。

### 3.4 输出处理接口

业务页面需要明确：

- 模型原始输出何时视为完整；
- `outputProcessor` 处理后的最终输出保存到哪里；
- 界面显示、数据库记录和导出结果如何保持一致；
- 用户确认前是否允许写入文档、PPT 或项目数据；
- 失败、取消和部分流式输出是否禁止写入。

### 3.5 流式和取消接口

流式页面需要提供：

- requestId 过滤；
- text delta 与 tool/protocol 事件分离；
- 完成事件；
- 取消事件；
- 页面卸载和切换会话时的监听清理；
- outputProcessor 的最终执行时机。

增强插件不应在每个 token 到达时重复执行，也不能在取消后继续修改结果。

### 3.6 权限和安全接口

前端解析插件只用于交互提示，Rust 后端仍需检查：

- 插件已安装并启用；
- 当前场景已启用；
- Manifest 权限已经授权；
- entitlement、商品状态和版本吊销状态有效；
- 文档、文件或智能体资源属于当前允许范围；
- 插件无法读取 Credential 明文；
- 日志和审计不保存完整敏感正文或隐藏提示词。

### 3.7 审计与状态反馈接口

宿主应能显示和记录：

- 本次解析到多少增强步骤；
- 哪些贡献执行成功或失败；
- 插件失败后是否按原功能继续；
- 调用耗时和脱敏错误；
- 插件禁用、场景关闭、权限拒绝和版本切换。

界面可以显示“执行了 1 个增强步骤”，但不应展示完整内部提示词。

## 4. 各功能板块需要准备的宿主接口

| 功能板块 | 当前状态 | 需要准备或补齐的接口 | 建议 scene / feature |
| --- | --- | --- | --- |
| AI 助学目标理解 | 已接入并人工看到增强步骤 | 保持现有 before/after、AI/本地模板状态提示 | `learning / goal-understanding` |
| AI 助学计划生成 | 已接入并人工看到增强步骤 | 保持知识库上下文、模型输出和本地 fallback 区分 | `learning / learning-planner` |
| 普通 AI 对话 | 未完整接入 | 原始消息/有效消息分离、隐藏 system context、流式完成后处理、持久化一致性 | `global / ai-chat-message` |
| AI 资源中心外部 Agent | 未完整接入 | original content、effective content、externalAgentId、requestId、隐藏 Workflow 前缀、usage | `global / external-agent-chat` |
| 文档智能总结 | Feature 可用，增强宿主待接入 | 当前文档只读上下文、摘要请求、预览输出、用户确认写入 | `global / document-summary` |
| 文档通用 AI 操作 | 待接入 | noteId、title、selectedText/fullText、只读/写入权限、预览和确认接口 | `global / document-ai-action` |
| AI 助研论文检索 | 当前只有 Crossref 检索 | 仅 inputProcessor 可真实作用于 query；模型提示 Hook 需等待真实 AI 分析入口 | `research / paper-search` |
| AI 助研论文分析 | 尚无统一模型调用入口 | 论文元数据/摘要上下文、隐藏 prompt、分析输出、引用来源和持久化 | `research / paper-analysis` |
| PPT 素材理解 | 未接入 | 每次 run 解析一次贡献、共享隐藏提示、direct/chunk/merge 复用、最终六维稿后处理 | `teaching / ppt-understanding` |
| PPT 生成规划 | 未接入 | 六维理解稿输入、slide plan 输出、主题/密度约束、导出前安全校验 | `teaching / ppt-generation` |
| 课程知识图谱 | 未接入，且不是普通模型调用 | 只读节点/关系上下文、选择节点 ID、禁止任意 Cypher；需要 AI 解释时再进入模型流水线 | `teaching / course-graph-explanation` |
| 市场、审核、插件管理 | 不属于业务增强宿主 | 只负责分发、权限、版本、场景开关和审计，不调用模型流水线 | 不适用 |

## 5. 建议统一的宿主适配结果

后续可以在不改变现有 `pluginPipeline` 的前提下，由集成人员封装统一宿主适配器。建议返回结构如下；这是后续接口目标，不代表当前已经存在同名函数：

```ts
interface PluginHostPreparedCall<TInput> {
  context: PluginExecutionContext;
  originalInput: TInput;
  effectiveInput: TInput;
  hiddenPrompt: string;
  executedContributionIds: string[];
  warnings: string[];
}

interface PluginHostCompletedCall {
  rawOutput: string;
  finalOutput: string;
  uiContributions: ResolvedEnhancementContribution[];
  executedContributionIds: string[];
  warnings: string[];
}
```

统一适配器的价值是让各页面只负责提供场景和业务数据，避免普通 AI、外部 Agent、文档和 PPT 分别实现不同的插件行为。该封装必须复用 `runPluginPipelineBeforeModel` 和 `runPluginPipelineAfterModel`，不能建立第三套增强执行器。

### 5.1 建议分工索引

| 负责板块 | 复制给 Codex 的提示词 | 主要交付 |
| --- | --- | --- |
| 普通 AI 问答 | 任务 A | 隐藏上下文、流式前后处理和消息持久化一致性 |
| AI 资源中心/星辰 Agent | 任务 B | ExternalAgent 调用前后处理和凭据隔离 |
| 文档与声明式 Feature | 任务 C | 文档上下文、摘要预览、确认写入和 FeatureHost 增强 |
| PPT | 任务 D | direct/chunk/merge 共享增强和最终结果处理 |
| AI 助研 | 任务 E | Crossref 查询输入增强，明确当前非模型边界 |
| 课程知识图谱 | 任务 F | 只读图谱上下文和安全 AI 解释入口 |
| AI 助学/教学场景维护 | 任务 G | 保持已有接入并补充回归、模型与本地模板状态 |
| 总集成负责人 | 任务 H | 场景、权限、流式、安全和原功能统一回归 |

对于没有 AI 调用、没有数据处理扩展点的普通页面，不需要为了“全覆盖”强行调用 `pluginPipeline`。只有页面需要允许插件改变输入、补充上下文、处理 AI 输出或显示声明式 UI 时，才应提供宿主接口。

## 6. 所有任务共同遵守的接入契约

每位开发者都必须满足：

1. 调用模型或受控业务能力前执行 `runPluginPipelineBeforeModel`。
2. 模型完成后执行 `runPluginPipelineAfterModel`；取消、失败和流式未完成输出不能被标记为成功。
3. 用户在聊天气泡、会话历史、文档和审计界面中只能看到原始输入，不能显示插件提示词、内部上下文或贡献点协议。
4. 没有匹配插件、插件被禁用、场景关闭或权限不足时，原调用参数和结果必须保持兼容。
5. 插件解析或执行失败时采用 fail-open：保留原功能，同时显示简短警告并记录脱敏审计，不能让宿主功能整体不可用。
6. 传入稳定的 `scene`、`feature`、`sessionId`、`workspaceId` 和必要元数据；不得把 API Key、Token 或 Authorization 放入 metadata。
7. `inputProcessor` 只能影响本次模型输入，不能静默改写用户已经保存的原文。
8. `contextProvider`、`promptEnhancer` 必须作为隐藏上下文传递，不能拼入可见用户消息。
9. `outputProcessor` 的最终结果必须与界面显示、消息持久化和导出结果保持一致，不能出现前端显示一版、数据库保存另一版。
10. `uiContribution` 只有宿主明确支持时才渲染；内容必须来自受控声明式资源，不允许 HTML、脚本或任意组件注入。
11. 流式调用需要保证同一 requestId 的事件不串台，切换页面和取消时解除监听。
12. 不修改插件市场、账号、交易、PPT 引擎或其他无关架构。
13. 不执行 Git 提交或推送，不覆盖当前未提交修改。

建议使用以下现有场景和功能标识，避免多人同时扩展 `PluginScene`：

| 页面 | scene | feature |
| --- | --- | --- |
| 普通 AI 对话 | `global` | `ai-chat-message` |
| AI 资源中心外部智能体对话 | `global` | `external-agent-chat` |
| 文档智能总结/文档 AI 操作 | `global` | `document-summary` |
| AI 助研论文检索 | `research` | `paper-search` |
| PPT 素材理解 | `teaching` | `ppt-understanding` |
| PPT 生成规划 | `teaching` | `ppt-generation` |

## 7. 任务 A：普通 AI 对话接入

将以下内容原样交给负责普通 AI 对话的开发者：

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：只为普通 AI 对话接入现有 Manifest v3 增强插件流水线，不改造其他页面。

先检查：
- src/pages/ai/index.tsx
- src/pages/ai/MobileAiChat.tsx
- src/services/pluginPipeline.ts
- src/lib/api.ts 中 aiChatApi
- 对应 Tauri AI command/service、流式事件和消息持久化逻辑
- AI 助学中 runPluginPipelineBeforeModel/AfterModel 的现有接法

实现要求：
1. 每次发送消息前使用 scene="global"、feature="ai-chat-message" 解析增强贡献。
2. 传入 conversation/session ID、当前 requestId 和非敏感模型元数据。
3. 用户消息必须仍按原始文本保存和显示；inputProcessor 的结果仅供本次模型输入。
4. contextProvider/promptEnhancer 作为隐藏 system/developer context 传入后端，绝不能拼进聊天气泡。
5. 不允许前端直接把插件提示词作为第二条可见消息发送。
6. 流式文本正常显示；outputProcessor 只在完整回答结束后执行，并确保最终显示与数据库持久化一致。
7. 取消、错误、切换会话时不能运行成功态后处理，也不能让旧响应覆盖新会话。
8. 桌面和 MobileAiChat 行为一致，避免两套实现分叉。
9. 无匹配插件时请求体、历史消息和流式行为与修改前一致。
10. 记录贡献点执行审计，但不得记录完整对话、密钥或隐藏提示词。

至少测试：
- 无插件时行为不变；
- matching promptEnhancer 生效但不出现在用户消息中；
- inputProcessor 不改写已保存用户消息；
- outputProcessor 的显示和持久化一致；
- 禁用插件/场景后不执行；
- 流式取消和跨会话隔离；
- 插件执行失败时原聊天仍可继续；
- 桌面与移动端基本一致。

完成后运行 pnpm.cmd exec tsc --noEmit、pnpm.cmd build、cargo check、相关测试，并报告修改文件和人工验收步骤。不要提交或推送。
```

## 8. 任务 B：AI 资源中心外部智能体对话接入

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：只给 AI 资源中心的 ExternalAgent/星辰 Workflow 对话接入增强插件，不修改凭据协议和星辰鉴权格式。

重点文件：
- src/pages/ai-resources/index.tsx
- src/services/pluginPipeline.ts
- src/lib/api.ts 中 externalAgentApi
- src-tauri/src/commands/xingchen_agent.rs
- src-tauri/src/services/xingchen_agent.rs
- agent session/message/usage 数据结构与流式事件

实现要求：
1. sendMessage 前使用 scene="global"、feature="external-agent-chat"。
2. 插件只能获得 externalAgentId、sessionId、用户原始文本和脱敏元数据，不能获得 credentialId 对应的明文凭据。
3. 用户消息按原文保存；增强后的输入和提示词只用于 Provider 请求。
4. 对仅接受文本的星辰 Workflow，在 Rust 后端以清晰、隐藏的上下文前缀组合，不在前端拼接，不写入可见消息历史。
5. 不改变 Authorization、flow_id、动态 parameters、文件上传或错误码解析逻辑。
6. outputProcessor 在完整回答聚合后执行，最终消息、usage 和界面结果保持一致。
7. 调用记录增加非敏感的插件贡献点标识；不得记录密钥、Authorization 或完整隐藏上下文。
8. Mock Provider 与真实 Provider 都走同一宿主接入，但自动测试只使用 Mock。
9. 取消后停止后处理，多个会话不能串流串台。

至少测试：
- Mock Agent 的 promptEnhancer/inputProcessor/outputProcessor；
- 用户原始消息未变化；
- 星辰请求中包含增强上下文但日志和历史不包含；
- 凭据无法被插件读取；
- entitlement、商品启用和吊销检查仍有效；
- 取消、错误、限流和多会话隔离；
- 未启用插件时原 Workflow 请求结构不变。

运行 TypeScript、构建、cargo check/test。不得使用真实星辰密钥，不提交或推送。
```

## 9. 任务 C：文档与声明式 Feature 调用接入

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：让文档智能总结及其他声明式 feature 调用能够被 enhancement/hybrid 插件增强，同时保留现有权限和用户确认流程。

重点检查：
- src/pages/plugins/PluginFeatureHost.tsx
- src/components/editor/PluginToolbarButtons.tsx
- src/pages/notes/editor.tsx
- src-tauri/src/services/plugin_platform.rs 的 prepare/finish feature invocation
- plugin feature invocation command、文档读取/写入权限和审计

实现要求：
1. 文档总结使用 scene="global"、feature="document-summary"。
2. 对通用 PluginFeatureHost，根据 feature Manifest 的场景和 feature ID 构造稳定上下文，不为每个插件硬编码页面。
3. 表单原值、文档标题和正文只按已授权范围传入；插件不得读取未选择文档或凭据。
4. inputProcessor/promptEnhancer 只修改发送给绑定 Agent/Workflow 的任务内容，不改变编辑器原文。
5. outputProcessor 只处理摘要预览；必须在用户点击确认后才能写入文档。
6. 失败结果、插件警告和内部提示词不能插入文档。
7. 禁用或卸载 enhancement/hybrid 后立即停止增强，不残留监听器或缓存。
8. 继续由 Rust 检查 document.read、document.write、ai.invoke、credentials.use 等权限。

至少测试：
- 文档为空、未打开文档和权限拒绝；
- 摘要预览被增强但原文不变；
- 用户确认后才写入；
- 插件失败不插入错误文本；
- feature 与 hybrid 的 feature 部分都能被增强；
- 插件禁用/卸载后不再执行；
- 无 enhancement 时当前文档总结链不回归。

运行 tsc、build、cargo check/test。不要重构编辑器，不提交或推送。
```

## 10. 任务 D：PPT 素材理解与生成接入

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：只为 PPT 的 AI 素材理解和生成规划入口接入增强插件，不修改 ppt-master 渲染/导出引擎，不破坏 direct/chunk/merge 链路。

重点文件：
- src/pages/ppt-generation/index.tsx
- src/services/pluginPipeline.ts
- src/lib/pptUnderstandingPrompt.ts
- src/lib/pptChunkUnderstandingPrompt.ts
- src/lib/pptChunkUnderstandingWorkflow.ts
- src/lib/api.ts 中 pptMasterApi
- Rust ppt_master commands/services

实现要求：
1. 素材理解使用 scene="teaching"、feature="ppt-understanding"。
2. 生成规划使用 scene="teaching"、feature="ppt-generation"。
3. 每次用户级素材理解任务只解析一次增强贡献；不要因为 3 个 chunk 重复记录 3 次相同插件执行。
4. 增强提示应进入共享理解指令，再由 direct/chunk/merge 复用；不得污染素材正文和缓存内容。
5. 保持 requestKind=direct/chunk/merge 的真实区分、缓存键、取消 runId、失败块重试和分层 merge。
6. outputProcessor 仅作用于最终六维理解稿或最终生成规划，不能逐块修改中间结果导致合并失真。
7. 增强后的理解稿仍需通过现有六字段校验、泄漏检查和 PPT 质量链。
8. 不把插件提示、贡献点 ID、runId、cacheKey 或内部角色写入 MD/PPTX。
9. nativeQualityEnabled 和插件增强是两套独立开关，不能互相覆盖。

至少测试：
- 无插件时 direct/chunk/merge 测试保持不变；
- promptEnhancer 在 direct 和长素材链中只解析一次；
- 取消和失败重试不重复执行贡献；
- 最终六维字段完整；
- MD 导出和 PPT 计划无内部信息；
- 禁用 teaching 场景后完全不增强；
- 不触发 legacy fallback。

只使用 Mock 自动测试，不调用真实 AI。运行 tsc、PPT Node 测试、build、cargo check/test，不提交或推送。
```

## 11. 任务 E：AI 助研场景接入

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：按当前真实能力给 AI 助研接入增强插件，不得把 Crossref 检索伪装成大模型调用。

先确认：当前 src/pages/research-assistant/index.tsx 主要调用 research_search_papers 获取 Crossref 元数据，不读取 AI Provider，也没有模型回答阶段。

实现范围：
1. 论文检索使用 scene="research"、feature="paper-search"。
2. 当前检索链只允许执行 inputProcessor，对检索主题/关键词做受控文本处理；处理后仍需通过长度和非空校验。
3. contextProvider、promptEnhancer、outputProcessor 不得谎称在 Crossref 请求中生效，因为当前没有模型 prompt/output。
4. 如 uiContribution 宿主已有安全渲染能力，可用于显示声明式检索提示；否则保持不渲染并给出明确 warning。
5. 用户原始检索词仍显示在表单中；增强后的实际 query 可以在脱敏调试信息中说明，但不能静默保存为用户输入。
6. 不改变 Crossref URL 安全、超时、年份过滤、去重和排序逻辑。
7. 若产品要求增强论文总结或研究建议，应单独提出“新增 AI 分析调用”任务，本轮禁止伪造。

至少测试：
- inputProcessor 能影响发送给 Crossref 的 query；
- 表单原值不被改写；
- promptEnhancer 不会被错误执行并声称成功；
- 禁用插件/场景后原检索完全一致；
- 插件异常时仍按原查询检索；
- 日志不记录完整敏感查询或凭据。

运行 tsc、build、cargo check/test，不提交或推送。最终明确报告：AI 助研当前只完成检索输入增强，模型型增强仍未实现。
```

## 12. 任务 F：课程知识图谱场景接入

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：让“课程知识图谱”在需要 AI 解释所选知识点时可以使用现有增强插件系统；继续保持课程图谱 SQLite 只读，不开放任意 SQL、Cypher 或写入能力。

先检查：
- src/pages/course-graph/
- src-tauri/src/commands/course_graph.rs
- src-tauri/src/services/course_graph.rs
- src/services/pluginPipeline.ts
- 当前统一 AI Provider/ExternalAgent 调用入口

实现要求：
1. 先确认课程图谱当前是否存在真实 AI 解释调用。若没有，不能仅显示“增强插件已执行”；只有复用现有统一 AI Provider 创建受控解释调用后才接入模型型 Hook。
2. AI 解释使用 scene="teaching"、feature="course-graph-explanation"。
3. 前端只传递用户主动选择的 nodeId；Rust 后端通过参数化只读查询加载节点名称、正文和 RELATED_TO 摘要。
4. selectedResources 只能包含受控节点 ID，不允许插件提交文件路径、SQL 或 Cypher。
5. contextProvider/promptEnhancer 作为隐藏上下文传给统一 AI 服务，不显示在图谱详情或用户问题中。
6. 用户原始问题单独保存；inputProcessor 不能修改图谱数据库。
7. outputProcessor 只处理 AI 解释文本，不改变节点、关系或 SQLite 资源。
8. 保持展开、搜索、详情、RELATED_TO、缩放和画布交互不受影响。
9. 插件禁用、teaching 场景关闭或权限不足时，AI 解释按原始逻辑执行；如果原本没有 AI 解释入口，则明确显示未配置而不是伪造结果。
10. 禁止访问笔记双向链接图、其他课程数据库和任意 Cypher 接口。

至少测试：
- 只读节点上下文可以作为受控模型背景；
- 非法 nodeId、路径穿越和任意 SQL/Cypher 被拒绝；
- 用户问题不包含隐藏插件提示；
- 禁用插件后不增强；
- 图谱浏览功能无回归；
- Mock Provider 下可验证 before/after，自动测试不使用真实密钥。

运行 tsc、build、cargo check/test 和课程图谱相关测试，不提交或推送。最终区分“图谱浏览”与“AI 解释增强”，不能把两者混为一项能力。
```

## 13. 任务 G：AI 助学既有接入维护与教学场景准备

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：不重写 AI 助学，只复核已有 enhancement 接入、修正状态提示，并为后续 teaching 场景复用形成稳定参考实现。

重点文件：
- src/pages/learning-assistant/index.tsx
- src/services/pluginPipeline.ts
- AI 助学 goal-understanding 和 learning-planner 调用位置
- 本地模板 fallback、知识库上下文和模型调用状态

实现要求：
1. 保持 scene="learning"、feature="goal-understanding" 和 feature="learning-planner" 不变。
2. 确认 before/after 只围绕真实 AI 调用执行；如果本地模板只解析了插件但没有使用 promptEnhancer，界面必须显示“已解析但本地模板未使用”，不能显示成增强已影响模型结果。
3. inputProcessor 不能覆盖用户填写的学习目标和项目数据。
4. contextProvider/promptEnhancer 只能进入模型上下文，不能写入学习项目 JSON、成绩、错题或页面可见原始输入。
5. outputProcessor 处理后的结果必须通过现有目标理解/计划结构校验，再保存到当前学习项目。
6. 本地模板 fallback、星火调用失败和插件执行失败必须分别显示，不能互相掩盖。
7. 多学习项目之间的插件执行上下文、计划和成绩保持隔离。
8. 为未来 teaching 页面整理可复用接法，但本轮不虚构尚不存在的教师模型功能。

至少测试：
- AI 模型模式下 enhancement 真正进入 prompt；
- 本地模板模式不虚假显示模型增强成功；
- 禁用插件/learning 场景后行为恢复；
- 目标理解和计划生成均只执行一次匹配贡献；
- 项目切换、重启恢复和 fallback 不丢数据；
- 插件提示词和凭据不进入项目持久化。

运行 tsc、build、cargo check/test 和 AI 助学相关测试，不提交或推送。
```

## 14. 任务 H：统一集成与回归验收

以下任务必须在 A-G 合并后由单独集成人员执行，避免各业务开发者同时修改公共入口：

```text
工作目录：D:\ag\firstwork\Pomegranate

目标：只做 pluginPipeline 多场景集成验收和最小冲突修复，不增加新功能。

检查：
1. 普通 AI 对话、外部 Agent、文档、PPT、AI 助研和既有 AI 助学使用同一个 pluginPipeline。
2. 各场景 feature ID 稳定，没有同名冲突。
3. 插件管理中的 global/learning/research/teaching 开关与运行时解析一致。
4. feature、enhancement、hybrid 均能解析；hybrid 的独立页面和增强贡献互不影响。
5. 没有插件时所有调用行为与基线兼容。
6. 插件禁用、权限拒绝、卸载和版本回滚后运行状态立即刷新。
7. 多插件优先级、runsBefore/runsAfter、冲突和循环依赖仍由 Rust 统一处理。
8. 用户消息、文档、PPT、审计和日志中不出现隐藏插件提示词、凭据或内部协议。
9. 流式事件、取消和切换会话不发生串台。
10. AI 助研只把已真实支持的 inputProcessor 标记为生效。

补充跨场景测试夹具，使用声明式资源和 Mock Provider，不使用真实凭据或真实收费调用。

运行：
- pnpm.cmd exec tsc --noEmit
- pnpm.cmd build
- cargo fmt --manifest-path src-tauri\Cargo.toml --check
- cargo check --manifest-path src-tauri\Cargo.toml
- cargo test --manifest-path src-tauri\Cargo.toml
- git diff --check

GUI 无法自动点击的项目标记 MANUAL，提供最短验收步骤，不提交或推送。
```

## 15. 建议实施顺序

1. A 普通 AI 对话。
2. B 外部智能体对话。
3. C 文档与 FeatureHost。
4. D PPT。
5. E AI 助研检索。
6. F 课程知识图谱。
7. G AI 助学维护与教学场景准备。
8. H 统一回归。

不要让多名开发者同时修改 `src/services/pluginPipeline.ts`、`src/types/index.ts` 或公共 API 类型。若确需修改，由集成人员先定义兼容接口，再由各业务任务调用。

## 16. 本文档交付判定与团队使用方式

本文档已经覆盖本次交付要求：

- 说明了当前已经验证可行的插件技术路线，以及 `.firstwork-plugin` 与普通任意 ZIP 的区别。
- 说明了开发者上传、Manifest v3 预检、审核、市场发布、获取、权限确认、安装和运行的完整生命周期。
- 分别说明了 `feature`、`enhancement`、`hybrid` 的用途、已验证程度和当前限制。
- 明确了业务页面接入插件系统必须准备的场景标识、原始输入、隐藏上下文、输出处理、流式取消、权限和审计接口。
- 建立了各功能板块的当前状态、缺失接口和推荐 `scene / feature` 对照表。
- 为普通 AI 对话、外部智能体、文档、PPT、AI 助研、课程知识图谱、AI 助学及统一回归提供了可直接复制给 Codex 的任务提示词。
- 明确区分了“插件包能够安装”和“业务页面已经接入增强流水线”，避免把未接入页面误报为插件已经生效。

团队执行时按以下方式分发：

1. 总集成人员保留本文档作为统一契约，不让各板块自行定义第二套插件接口。
2. 每位板块负责人复制第 7 至 13 节中与自己模块对应的完整提示词，发送给在该模块工作区运行的 Codex。
3. 板块负责人只提交本模块的宿主接入、测试和人工验收结果，不修改市场供应链和其他业务页。
4. 如确需调整 `pluginPipeline`、公共类型或 Tauri 公共命令，由总集成人员先统一设计兼容接口，再由各板块使用。
5. 所有板块完成后，单独执行第 14 节的统一集成与回归验收。

最终验收不能只看插件是否成功安装，还要逐项确认：

- 对应业务页确实调用了统一 `pluginPipeline`。
- 插件启用、场景匹配和权限检查真实生效。
- 原始用户内容与隐藏增强上下文相互隔离。
- 插件禁用、权限拒绝或执行失败时，原功能仍能正常使用。
- 流式、取消、持久化和审计没有串台或泄漏内部信息。
- 测试通过情况与 GUI 人工验证情况分别报告，不把未点击验证的项目写成通过。
