# Pomegranate 插件规范制作流程

本文依据当前 Pomegranate 源码、Manifest v3 三个安全示例、星火平台前期设计文档，以及“文档智能总结”插件的实际制作与打包过程编写。源码规则发生变化时，应优先重新核对当前代码，不得只沿用本文的历史结论。

## 1. 插件系统概览

### 1.1 Manifest 是什么

`manifest.json` 是插件的能力声明。它描述插件 ID、版本、分类、运行时、适用场景、权限、贡献点、资源路径、完整性和签名状态。Manifest 只描述能力，不承载用户密钥，也不是任意代码入口。

当前正式插件使用 `schemaVersion: 3`。Rust 后端会再次校验 Manifest、资源路径、权限、运行时策略、依赖、冲突和应用版本，不能只依赖前端按钮隐藏。

### 1.2 uiSchema 是什么

`uiSchema` 是声明式功能表单，通常位于 `ui/feature.json`。宿主组件 `PluginFeatureHost` 读取它并渲染输入控件。插件只提供 JSON 数据，不注入 HTML、CSS、JavaScript 或任意 Tauri command。

### 1.3 `.firstwork-plugin` 是什么

`.firstwork-plugin` 是当前正式插件安装包。它本质上是受控 ZIP，由 `scripts/plugin-package.mjs` 生成，包内自动增加 `checksums.json`。安装前后会校验 Manifest、资源、禁止文件、疑似密钥和 SHA-256。

### 1.4 安全凭据系统是什么

用户在 AI 资源中心保存 APPID、API Key、API Secret 或 Token。SQLite 只保存凭据元数据和引用，Windows 上密钥文件由 Rust 使用 DPAPI 保护。前端和插件只能看到 `credentialId`、掩码提示或“已配置”状态，不能读取完整密钥。

### 1.5 星辰 Workflow 如何被插件调用

当前真实调用链如下：

```text
PluginFeatureHost 声明式表单
→ 用户选择 AI 资源中心 ExternalAgent
→ plugin_feature_invoke_xingchen
→ PluginPlatformService 校验插件/运行时/权限
→ XingchenAgentService 读取安全凭据并调用 Workflow
→ 解析文本、Markdown、JSON 或受控文件输出
→ 返回插件功能页并写入审计/调用记录
```

插件包不保存 `credentialId`、ExternalAgent ID、Flow ID 或密钥。用户在运行时选择已配置服务。

### 1.6 为什么禁止任意代码和真实密钥

市场插件若能执行任意 JavaScript、Python、Shell 或二进制，就可能绕过权限系统、读取用户文件或凭据。正式 v3 包因此只允许声明式资源和受控星辰运行时；包内可执行脚本、`.env`、私钥和疑似密钥会被打包器拒绝。

## 2. 插件分类选择

### 2.1 `feature`

适合独立、由用户主动打开和提交的功能，例如文档总结、教案生成或题目解析。

- 必须声明 `contributes.features`。
- 每个 feature 必须提供 `uiSchema`。
- 可使用 `declarative-ui` 做本地模板输出，或使用 `xingchen-workflow`/`xingchen-agent` 做受控外部调用。
- 功能路由由宿主提供：`/plugins/:pluginId/features/:featureId`。
- 不应在后台静默消耗外部额度。

常见误用：把只需增强提示词的插件做成独立功能页，或在 feature 资源中加入脚本。

### 2.2 `enhancement`

适合增强现有 AI 助学、助研或教学流程，例如追加上下文、规范输出结构。

- 必须声明 `contributes.enhancements`。
- handler 必须为 `declarative` 并指向包内文本资源。
- 通常使用 `prompt-pack`。
- 通常需要 `ai.context.augment`。
- 通过 `pluginPipeline` 的输入处理、上下文、提示词或输出阶段执行。

常见误用：让 enhancement 隐式调用付费 Workflow。需要显式外部调用时优先设计为 `feature` 或 `hybrid`。

### 2.3 `hybrid`

适合既有独立功能页，又增强现有 AI 流程的插件。

- 必须同时声明 feature 与 enhancement。
- 权限是两部分能力的并集，但仍要逐项最小化。
- 用户应能理解哪些行为会访问外部服务，哪些只是本地提示增强。

### 2.4 简单决策流程

```text
需要独立页面吗？
├─ 否 → 只增强现有 AI 调用 → enhancement
└─ 是
   ├─ 不增强现有流程 → feature
   └─ 同时增强现有流程 → hybrid
```

## 3. 开发前准备

1. 在 `Pomegranate` 目录确认 `package.json` 的应用版本和脚本。
2. 查看 `dev-plugins/PLUGIN_V3_EXAMPLES.md` 和三个 v3 示例。
3. 确认当前 Manifest schema 是 v3。
4. 从 `src/types/index.ts` 和 Rust `models/plugin_platform.rs` 确认运行时、场景、贡献点类型。
5. 从 `scripts/plugin-package.mjs` 确认打包器允许的权限、运行时、文件类型和命令参数。
6. 从 `PluginFeatureHost.tsx` 确认可用表单字段和输出类型。
7. 确认是否真正需要星辰 Workflow；本地固定模板优先使用 `declarative-ui`。
8. 确认是否真的需要文件访问。文件上传与“读取并提取文件正文”不是同一能力。
9. 凭据一律由 AI 资源中心配置，插件只声明使用权限。

当前正式打包器接受的运行时：

- `declarative-ui`
- `prompt-pack`
- `xingchen-agent`
- `xingchen-workflow`

`legacy-js`、`xingchen-mcp` 和 `mcp-connector` 虽可能出现在共享类型中，但当前 v3 打包器不接受它们作为正式包运行时。以打包器结果为准。

## 4. 标准目录结构

### 4.1 feature

```text
plugin-id/
├── manifest.json
├── README.md
├── ui/
│   └── feature.json
├── assets/                 # 可选，只放静态资源
└── examples/               # 可选，安全示例输入/输出
```

### 4.2 enhancement

```text
plugin-id/
├── manifest.json
├── README.md
└── prompts/
    └── enhancement.md
```

### 4.3 hybrid

```text
plugin-id/
├── manifest.json
├── README.md
├── ui/
│   └── feature.json
└── prompts/
    └── enhancement.md
```

### 4.4 允许与禁止进入包的内容

允许：JSON、Markdown、纯文本、YAML、XML、CSV 和必要静态资源。

禁止：

- JavaScript、MJS、CJS、Python、PowerShell、BAT、CMD。
- EXE、DLL、SO、DYLIB 等可执行文件或动态库。
- `.env`、私钥、证书私钥文件、真实凭据。
- `node_modules`、`target`、`dist`、`build`、缓存和日志。
- 符号链接、路径穿越、绝对路径配置。
- 普通运行数据库和用户数据。

## 5. Manifest 编写规范

### 5.1 必填字段

当前打包器至少要求：

- `schemaVersion`
- `id`
- `name`
- `version`
- `authorId`
- `classification`
- `runtimeKind`
- `permissions`
- 与分类匹配的 `contributes`
- feature 的 `uiSchema`
- 根目录 `README.md`

Rust 安装预检还要求 `supportedScenes` 非空，并校验贡献点 ID、资源路径、重复权限和应用兼容性。

### 5.2 ID 与版本

- ID 只使用小写字母、数字、点、横线和下划线。
- 推荐反向域名，例如 `com.vendor.plugin-name`。
- 版本使用语义化版本，例如 `1.0.0`。
- 更新必须保持同一 ID，并递增版本；不要用名称充当身份。

### 5.3 关键字段说明

- `classification`：`feature`、`enhancement` 或 `hybrid`。
- `runtimeKind`：从当前打包器允许列表选择。
- `source`：本地开发和直接安装使用 `local`；不要把它伪装成已通过市场审核的 `marketplace`。
- `handler`：若声明，只能为 `kind: declarative`，resource 必须位于包内。
- `permissions`：只申请功能实际使用的权限。
- `supportedScenes`：当前支持 `global`、`learning`、`research`、`teaching`。
- `defaultActivation`：定义全局和场景默认启用状态。
- `integrity`：源码中可使用 `sha256: null`，打包器会生成独立 `checksums.json`。
- `signature`：未接入可信签发时必须是 `unsigned`，不能伪造 `valid`。

### 5.4 feature 最小安全示例

```json
{
  "schemaVersion": 3,
  "id": "com.example.safe-feature",
  "name": "安全功能示例",
  "version": "1.0.0",
  "authorId": "example-developer",
  "minAppVersion": "1.8.0",
  "classification": "feature",
  "runtimeKind": "declarative-ui",
  "source": "local",
  "activationEvents": ["onFeature:safe-feature"],
  "supportedScenes": ["global"],
  "defaultActivation": { "global": true, "scenes": {} },
  "permissions": [],
  "dependencies": {},
  "conflictsWith": [],
  "contributes": {
    "features": [{
      "id": "safe-feature",
      "title": "安全功能示例",
      "scenes": ["global"],
      "capabilities": ["feature"],
      "uiSchema": "ui/feature.json",
      "handler": { "kind": "declarative", "resource": "ui/feature.json" }
    }],
    "agents": [], "commands": [], "views": [], "tools": [],
    "enhancements": [], "settings": false
  },
  "integrity": { "sha256": null },
  "signature": { "status": "unsigned", "signer": null }
}
```

## 6. uiSchema 编写规范

### 6.1 当前支持的字段

| 类型 | 用途 |
| --- | --- |
| `text` / `string` | 单行文本 |
| `textarea` / `multiline` | 多行文本 |
| `integer` | 整数 |
| `number` | 数值 |
| `select` | 枚举选择 |
| `switch` / `boolean` | 布尔开关 |
| `json` | JSON 对象或数组文本，提交前解析 |
| `file` | 选择单文件并交给后端上传 Workflow |
| `files` | 选择多文件并交给后端上传 Workflow |

字段可使用 `key` 或 `id`。推荐统一使用 `key`。还支持：

- `label`
- `required`
- `placeholder`
- `description`
- `defaultValue`
- `rows`
- `options`
- `sensitive`

`sensitive: true` 或密钥类字段名会被拒绝。凭据不能通过 uiSchema 收集。

### 6.2 参数映射

对 `xingchen-workflow` feature，当前宿主会把非空字段按原 key 写入 Workflow `parameters`：

```json
{
  "AGENT_USER_INPUT": "用户填写的文本",
  "audience": "student"
}
```

字段 key 必须与星辰 Workflow 开始节点字段完全一致。显示名称不影响真实参数名。

### 6.3 `AGENT_USER_INPUT` 与动态参数

旧工作流通常使用 `AGENT_USER_INPUT`。当前框架也支持多个动态字段，但不支持在 uiSchema 中用字符串模板把多个可视字段合成为一个 `AGENT_USER_INPUT`。

当 Workflow 只接受一个文本参数，而产品想展示“类型、长度、语言、正文”等多个概念时，稳定降级方式是使用一个多行字段，并在 placeholder、description 或 README 中提供填写模板。不要自行发明 `templateMapping` 等未实现字段。

### 6.4 输出类型

当前 `output.kind` 支持：

- `text`
- `markdown`
- `json`
- `docx-base64`
- `file-base64`

`json` 会进行严格兼容解析；文件输出由 Rust 校验 Base64、文件名、大小、扩展名和 DOCX 文件头。文件输出必须申请并获得 `files.writeSelected`。

`declarative-ui` 可使用 `outputTemplate` 生成本地演示文本；这不是 AI 调用，界面会与真实 Provider 结果区分。

## 7. 星辰服务接入

### 7.1 用户配置

1. 用户进入 AI 资源中心。
2. 创建讯飞星辰凭据，敏感值只提交 Rust 后端。
3. 创建 ExternalAgent/Workflow 配置，填写 Flow ID 和协议参数。
4. 测试并启用配置。
5. 插件功能页只选择可用 ExternalAgent。

### 7.2 标识边界

- `credentialId`：安全凭据元数据引用，不是密钥。
- `externalAgentId`：Pomegranate 中的一条已配置外部智能体记录。
- Flow ID：属于 ExternalAgent 的星辰资源配置，不应硬编码进通用插件包。
- Agent ID/Bot ID：只有所选协议需要时才进入 ExternalAgent 配置。

### 7.3 前后端职责

- 前端：渲染表单、选择 ExternalAgent、做必填和基础类型校验、展示结果。
- Rust Core：校验插件和权限、读取安全凭据、上传文件、构造请求、解析响应、映射错误、写审计。
- `XingchenAgentService`：执行统一 Workflow 调用，不允许插件自己拼鉴权头或直接请求 Endpoint。

### 7.4 输出处理

- 文本/Markdown：读取当前 Workflow 适配器支持的内容字段并展示。
- JSON：支持普通 JSON、代码块 JSON和二次字符串化 JSON的现有兼容解析。
- Base64 文件：只在声明文件输出并授权写文件时处理；失败时不把 Base64 原文显示给用户。

## 8. 权限最小化

当前打包器允许的权限如下。并非所有插件都应申请全部权限。

| 权限 | 用途 | 常见插件 |
| --- | --- | --- |
| `notes.read` / `notes.write` | 读取或写入笔记 | 笔记增强功能 |
| `document.read` / `document.write` | 读取或写入当前文档 | 编辑器插件 |
| `tasks.read` / `tasks.write` | 读取或修改待办 | 任务插件 |
| `ai.invoke` | 发起 AI 能力调用 | 星辰 feature |
| `ai.context.read` | 读取受控 AI 上下文 | 上下文插件 |
| `ai.context.augment` | 增强现有 AI 上下文 | enhancement/hybrid |
| `ai.session.read` | 读取受控会话信息 | 会话增强插件 |
| `ui.editor.toolbar` | 增加编辑器工具栏入口 | 编辑器插件 |
| `ui.chat.toolbar` / `ui.chat.panel` | 增加聊天入口或面板 | 聊天插件 |
| `planning.files.read` / `planning.files.write` | 访问受控规划工作区 | Planning 插件 |
| `network.request` | 受控通用网络访问 | 非星辰连接器，需高风险审核 |
| `files.readSelected` | 读取用户主动选择的文件 | 真正需要本地文件读取的插件 |
| `files.writeSelected` | 写入用户确认的文件 | Base64 文件输出 |
| `prompts.register` | 注册 Prompt | Prompt 包 |
| `views.register` | 注册受控视图 | 声明式界面插件 |
| `mcp.connect` | 连接受控 MCP | MCP 连接器 |
| `credentials.use` | 后端使用用户选定凭据 | 星辰 feature |
| `network.xingchen` | 访问星辰网络服务 | 星辰 feature |
| `agents.invoke` | 调用已配置 ExternalAgent | 星辰 feature |

权限不足会被 Rust 拒绝；权限过多会增加审核风险。`credentials.configure` 是官方设置能力，不在当前 v3 打包器允许列表，普通插件不得申请。

## 9. 安全要求

1. 正式 handler 只使用 `declarative`。
2. 不使用 `legacy-js`，不在插件包携带脚本或二进制。
3. Manifest、uiSchema、README 和示例中不得包含真实密钥、鉴权头或私钥。
4. Endpoint 由受控 ExternalAgent 配置和 Rust 网络策略管理。
5. 文件路径必须来自用户主动选择，不硬编码绝对路径。
6. 上传和生成文件要限制大小、文件名、扩展名和文件头。
7. 日志只记录插件 ID、功能 ID、请求 ID、状态、耗时和脱敏错误，不记录完整凭据。
8. 插件包使用 SHA-256 检查资源一致性；安装前后内容改变会被发现。
9. 当前可信公钥签名服务尚未完善。`unsigned` 包只能在用户看到风险提示并明确确认后安装，不能把它描述为已验签。
10. 开发者中心的旧市场扫描与 v3 安装预检不是同一条链，不能用旧扫描结果替代 v3 安全检查。

## 10. 本地制作和打包

以下命令在 Windows PowerShell 的 `Pomegranate` 目录执行。

### 10.1 创建输出目录

```powershell
New-Item -ItemType Directory -Force dev-plugins\packages | Out-Null
```

### 10.2 打包

```powershell
pnpm.cmd plugin:pack -- dev-plugins\v3-document-summary dev-plugins\packages\com.pomegranate.demo.document-summary-1.0.0.firstwork-plugin
```

真实 CLI 形式为：

```text
plugin-package.mjs pack <plugin-directory> [output]
```

省略 output 时，安装包会生成在插件源码目录的上一级。团队交付建议显式写入 `dev-plugins/packages`。

### 10.3 离线验证

```powershell
pnpm.cmd plugin:verify -- dev-plugins\packages\com.pomegranate.demo.document-summary-1.0.0.firstwork-plugin
```

真实 CLI 形式为：

```text
plugin-package.mjs verify <plugin-file>
```

### 10.4 查看包内容

```powershell
Add-Type -AssemblyName System.IO.Compression.FileSystem
$path = (Resolve-Path dev-plugins\packages\com.pomegranate.demo.document-summary-1.0.0.firstwork-plugin).Path
$zip = [System.IO.Compression.ZipFile]::OpenRead($path)
$zip.Entries | Select-Object FullName, Length
$zip.Dispose()
```

### 10.5 查看安装包哈希

```powershell
Get-FileHash -Algorithm SHA256 dev-plugins\packages\com.pomegranate.demo.document-summary-1.0.0.firstwork-plugin
```

`plugin:verify` 还会逐文件验证包内 `checksums.json`。包级哈希和包内逐文件哈希用途不同，都建议保留。

### 10.6 校验失败处理

- Manifest 错误：按错误定位字段，不要删除校验规则。
- 资源不存在：修正相对路径和大小写。
- 疑似密钥：删除真实值，改用 AI 资源中心凭据引用。
- 禁止文件：从包目录移除脚本、二进制、`.env` 或私钥。
- 权限不存在：检查拼写和当前打包器允许列表。
- 校验和不匹配：重新从干净源码打包，不手工修改安装包。

## 11. 开发者模式上传和人工验收

### 11.1 启动

按项目当前启动文档启动 Pomegranate。测试时建议使用独立数据目录，避免污染正式用户数据。

### 11.2 开发目录调试

插件管理页当前提供“安装开发目录”。选择插件源码目录可以调试资源，但这不等于安装包预检，也不代表市场审核通过。

### 11.3 正式本地包安装

1. 打开“插件”。
2. 点击“安装插件包”。
3. 选择 `.firstwork-plugin`。
4. 查看 Manifest、分类、运行时、场景、文件数、签名、SHA-256 和权限变化。
5. 对 `unsigned` 风险进行明确确认。
6. 安装并启用插件。
7. 在“插件功能入口”打开 feature。
8. 星辰插件选择 AI 资源中心中可用 Workflow。
9. 填写表单并调用。
10. 在插件详情查看审计日志和错误日志。

### 11.4 更新、回滚与卸载

- 更新：使用同一插件 ID、更高版本重新打包并从“安装插件包”进入安全流水线；重点核对新增权限。
- 回滚：在插件详情的“版本与启用范围”中选择保留的历史版本并确认回滚。
- 停用：关闭全局或场景启用状态，确认 feature 不再可用。
- 卸载：使用插件列表中的卸载操作，确认功能入口和运行状态被清理。

### 11.5 本地市场发布链

开发者中心现在按扩展名明确分流：`.zip` 进入 Manifest v2 兼容链，`.firstwork-plugin` 进入 Manifest v3 的 `PluginPlatformService` 预检链。不得改扩展名混用解析器。

Manifest v3 推荐流程：

1. 在开发者中心创建商品草稿，选择“上传插件版本”。
2. 选择 `.firstwork-plugin`，核对包格式、classification、贡献点、场景、权限、凭据要求、扫描结果和 SHA-256。
3. 预检通过后提交审核；失败报告必须修复后重新打包上传。
4. 管理员在审核中心复核真实插件包，批准后锁定该版本及包 SHA-256。
5. 切换买家账号，在 AI 应用市场获取并安装；安装前再次确认权限。
6. 安装过程会再次复验哈希和安全边界，并复用插件管理页相同的 v3 安装核心。
7. feature 从插件管理的功能入口打开；enhancement/hybrid 在插件管理中设置启用场景。

当前仍是本地模拟市场，批准不代表正式数字签名或远程市场发布；`unsigned` 包会明确提示可信公钥验签尚未接入。

## 12. 测试与验收清单

### Manifest

- [ ] schemaVersion 为 3。
- [ ] ID 稳定、合法且唯一。
- [ ] 版本符合语义化版本。
- [ ] classification 与贡献点一致。
- [ ] runtimeKind 被当前打包器允许。
- [ ] source 与交付路径一致。
- [ ] supportedScenes 非空。
- [ ] handler 仅为 declarative。

### uiSchema

- [ ] feature 存在有效 uiSchema。
- [ ] 字段 key 与 Workflow 开始节点一致。
- [ ] 必填、默认值、枚举和说明符合实际。
- [ ] 未使用宿主不支持的模板合成字段。
- [ ] 输出类型符合 Workflow 返回格式。

### 权限与安全

- [ ] 权限完整但没有多余项。
- [ ] Manifest 和资源中没有凭据明文。
- [ ] 包内没有脚本、二进制、`.env`、私钥或缓存。
- [ ] 没有本机绝对路径。
- [ ] 文件能力有真实读取/上传/保存链，而不是只提供路径字符串。
- [ ] 调用页面说明数据去向和额度消耗。

### 打包与安装

- [ ] `plugin:pack` 成功。
- [ ] `plugin:verify` 成功。
- [ ] 包内存在 `checksums.json`。
- [ ] `Get-FileHash` 已记录包级 SHA-256。
- [ ] 安装预检显示正确权限和签名状态。
- [ ] 未授权前不能调用。
- [ ] 安装并启用后出现功能入口。
- [ ] 开发者中心识别为 `v3-firstwork-plugin`，未进入 v2 解析器。
- [ ] 审核记录关联具体插件 ID、版本和 SHA-256。
- [ ] 管理员批准后包哈希锁定，市场安装前复验一致。
- [ ] 未批准或被驳回版本不能从市场安装。

### 调用与输出

- [ ] 可选择正确 ExternalAgent。
- [ ] Workflow 参数与 schema 一致。
- [ ] 成功响应可解析。
- [ ] 错误码和建议不会泄漏凭据。
- [ ] JSON/Markdown/文件输出符合声明。
- [ ] 审计和调用记录可见。

### 生命周期与回归

- [ ] 禁用后入口不可用。
- [ ] 更新时显示权限变化。
- [ ] 回滚后版本和资源恢复。
- [ ] 卸载后入口和状态清理。
- [ ] 原有三个 v3 示例仍能验证。
- [ ] Pomegranate 原有功能不受影响。

## 13. 常见问题

### Manifest 校验失败

先看打包器的明确字段错误。常见原因是 schemaVersion 不是 3、ID 含大写、分类与贡献点不匹配、场景为空或资源路径不存在。

### 找不到功能入口

确认插件已安装并启用，feature 场景与默认激活一致，`uiSchema` 可读取，路由由宿主解析。检查插件详情的运行时策略和错误日志。

### 无法选择 Workflow

AI 资源中心必须存在已启用、凭据有效、授权有效、未吊销且协议受支持的 ExternalAgent。Mock 与真实 Provider 会在界面中明确区分。

### 凭据不存在

回到 AI 资源中心重新绑定凭据。不要把密钥补进 Manifest 或 uiSchema。

### 权限不足

确认 Manifest 声明的权限与安装时批准权限一致。星辰 feature 当前需要 `credentials.use`、`agents.invoke`、`network.xingchen` 和 `ai.invoke`。

### Workflow 参数不匹配

核对星辰开始节点的真实字段 key、类型和必填状态。显示标签不能代替参数 key。

### `AGENT_USER_INPUT` 不匹配

若开始节点字段不是 `AGENT_USER_INPUT`，应修改 uiSchema 的 key 并重新打包。不要只改 label，也不要依赖未实现的远程 schema 自动发现。

### JSON 解析失败

确认 Workflow 返回的是 JSON、代码块 JSON 或可兼容的二次字符串化 JSON。不要把普通错误文本声明为 JSON 输出。

### Base64 文件失败

检查 output.kind、`file_content`、`file_name`、Base64 完整性、大小、扩展名和文件头。文件输出还必须有 `files.writeSelected`。

### 安装包包含禁止文件

从插件源码目录移除脚本、二进制、私钥、`.env`、缓存和构建产物后重新打包。

### 疑似密钥扫描误报

不要降低扫描规则。先把示例值改成清晰占位符，避免使用像真实密钥的长随机字符串。若仍误报，记录文件和脱敏片段后由平台维护者评估。

### 更新或回滚失败

确认 ID 未改变、版本合法、旧版本仍保留、内容哈希有效且新增权限已确认。不要直接覆盖安装目录。

## 14. “文档智能总结”插件实例

### 14.1 为什么选择 feature

该插件由用户主动选择 Workflow、输入正文并点击“开始总结”，属于明确的独立调用，不应后台自动消耗额度，因此选择 `feature`。

### 14.2 实际目录

```text
dev-plugins/v3-document-summary/
├── manifest.json
├── README.md
├── ui/feature.json
└── examples/sample-input.md
```

### 14.3 关键 Manifest 字段

- ID：`com.pomegranate.demo.document-summary`
- 版本：`1.0.0`
- classification：`feature`
- runtimeKind：`xingchen-workflow`
- source：`local`
- handler：`declarative`
- 场景：learning、research、teaching

### 14.4 uiSchema 与参数映射

当前宿主不支持把“总结类型、长度、语言、关注重点、正文”五个可视字段模板合成为一个参数，因此插件使用单一必填多行字段：

```json
{
  "AGENT_USER_INPUT": "包含总结要求和文档正文的完整文本"
}
```

这种设计能直接兼容使用 `AGENT_USER_INPUT` 的普通星辰文本 Workflow，并避免 20354 参数 schema 错误。

### 14.5 权限

- `credentials.use`
- `agents.invoke`
- `network.xingchen`
- `ai.invoke`

没有文件读取、文件写入、通用网络或上下文增强权限。

### 14.6 打包命令

```powershell
pnpm.cmd plugin:pack -- dev-plugins\v3-document-summary dev-plugins\packages\com.pomegranate.demo.document-summary-1.0.0.firstwork-plugin
pnpm.cmd plugin:verify -- dev-plugins\packages\com.pomegranate.demo.document-summary-1.0.0.firstwork-plugin
```

### 14.7 当前限制

- 不支持从多个表单控件合成一个 Workflow 参数。
- 不支持直接读取 DOCX/PDF 正文。
- 不包含 Flow ID 或凭据，必须由用户选择 AI 资源中心配置。
- 已接通本地开发者上传、审核批准、市场获取和 v3 安装链；仍需人工完成桌面端按钮与 Workflow 绑定验收。
- 未签名，只适合开发和人工验收。

## 15. 文档与代码差异

| 项目 | 前期文档描述 | 当前真实实现 | 制作插件时采用的规则 |
| --- | --- | --- | --- |
| 凭据输入 | 功能页可填写 API Key/API Secret 或选择 credentialId | FeatureHost 只选择 AI 资源中心 ExternalAgent，前端不收集密钥 | 用户先在 AI 资源中心配置，插件只提交 externalAgentId |
| Flow ID | 可放 configurationSchema 或安装时填写 | Flow ID 属于 ExternalAgent 配置 | 插件包不写死 Flow ID |
| 多字段输入 | uiSchema 多字段可构造 parameters | 已支持按 key 直接构造，但不支持模板合成 | 需要单一参数时使用一个 multiline 字段和填写模板 |
| 文件输入 | 文档设想可上传文件 | 当前 file/files 是后端上传 Workflow，不是读取并提取文档正文 | 本例只粘贴正文，不伪装文件解析能力 |
| 输出类型 | 主要描述 text、json、docx-base64 | 还支持 markdown、file-base64 | 本例使用 markdown；文件输出遵循额外权限和校验 |
| Manifest 来源 | 市场插件示例使用 marketplace | v3 本地开发包可使用 local；审核版本的分发来源由市场数据库单独记录 | 本例保持 local，只有管理员批准后才获得本地市场发布状态 |
| 市场上传 | 设想可导入 `.firstwork-plugin` | 开发者中心已按扩展名分流 v2 `.zip` 与 v3 `.firstwork-plugin` | v3 必须通过 PluginPlatformService 预检、审核哈希锁定和市场安装复验 |
| handler | 文档示例有时省略 feature handler | feature handler 可选；一旦声明只能 declarative | 本例显式声明 declarative，资源指向 uiSchema |
| 包完整性 | 描述 Manifest integrity 和平台签名 | 打包器生成 checksums.json；可信公钥签发尚未完成 | 使用逐文件 SHA-256，签名保持 unsigned |
| 运行时范围 | 共享类型包含更多 runtimeKind | 当前打包器只接受四种正式运行时 | 以打包器允许列表为准 |

## 维护原则

每次应用版本、Manifest 类型、FeatureHost 控件或打包器变化后，应重新运行三个现有示例和一个真实星辰 feature 的离线验证。若本文与代码冲突，以能通过当前 Rust 预检和当前 `plugin-package.mjs` 的实现为准，并同步更新本文差异表。
