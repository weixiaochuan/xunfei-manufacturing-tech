# 文档智能总结

## 功能

“文档智能总结”是用于验证 Pomegranate Manifest v3 插件全链路的声明式测试插件。它提供独立功能页面，允许用户选择 AI 资源中心中已经配置并启用的讯飞星辰 Workflow，粘贴总结要求与文档正文，并以 Markdown 展示返回结果。

本插件不会执行第三方 JavaScript、Python、Shell 或二进制程序，也不包含任何用户凭据。

## 插件类型

- `classification`：`feature`
- `runtimeKind`：`xingchen-workflow`
- `handler`：`declarative`
- 适用场景：`learning`、`research`、`teaching`
- 输出类型：Markdown

## 安装

1. 使用项目自带打包工具生成 `.firstwork-plugin`。
2. 启动 Pomegranate，进入“插件”页面。
3. 选择 Firstwork 插件包并查看预检结果。
4. 确认插件 ID、版本、权限、完整性摘要和未签名提示后安装。
5. 启用插件，再打开“文档智能总结”功能入口。

当前 Manifest v3 安装链位于插件管理页。开发者中心的商品版本上传仍采用旧市场 Manifest/ZIP 链，不能把两条链路视为已经完全打通。

## 配置讯飞 Workflow

1. 进入“AI 资源中心”。
2. 保存用户自己的讯飞星辰凭据。
3. 创建并启用一个“讯飞星辰 Workflow Open API v1”智能体配置。
4. 填写并核对该 Workflow 的 Flow ID。
5. 确认 Workflow 开始节点存在字符串字段 `AGENT_USER_INPUT`。
6. 回到插件页面，从“调用智能体”下拉框选择该配置。

插件只保存或提交 `externalAgentId`，无法读取凭据明文。实际网络请求由 Rust 后端通过现有 `XingchenAgentService` 发起。

## 输入方法

当前 `PluginFeatureHost` 会把每个表单字段按字段 key 原样写入 Workflow `parameters`，尚不支持用模板把多个可视字段合成为一个字段。为了兼容普通文本生成 Workflow，本插件使用一个必填长文本字段：

- 字段 key：`AGENT_USER_INPUT`
- 字段名称：总结要求与文档内容

建议按下面的结构填写：

```text
请根据以下要求总结文档：

总结类型：结构化总结
总结长度：标准
输出语言：中文
关注重点：（可选）

文档正文：
在这里粘贴需要总结的正文。
```

可使用 `examples/sample-input.md` 进行非敏感离线准备和人工调用测试。

## 输出

插件按 Markdown 结果展示。所选 Workflow 应把最终文本放在当前星辰适配器支持的响应内容字段中；Pomegranate 会复用现有 Workflow 响应解析和错误映射。

## 文件能力

本版本不提供直接选择 TXT、Markdown、DOCX 或 PDF 的入口。当前声明式 `file/files` 控件用于把用户选择的文件上传给 Workflow，并不等于安全读取文件后提取正文；平台也没有为该插件提供通用 DOCX/PDF 正文提取链。为避免误导，本插件只支持粘贴正文。

## 权限

- `credentials.use`：允许 Rust 后端代表插件使用选定凭据，插件看不到明文。
- `agents.invoke`：允许调用已配置的外部智能体或 Workflow。
- `network.xingchen`：允许受控访问讯飞星辰服务。
- `ai.invoke`：允许执行 AI 调用。

本插件不申请文件读写、任意网络、命令执行或上下文增强权限。

## 安全与费用说明

- 插件包不包含 API Key、API Secret、Token、Flow ID 或用户身份。
- 凭据由 AI 资源中心和 Rust 安全凭据服务管理。
- 表单正文会被发送到用户选择的讯飞星辰 Workflow。
- 调用可能消耗用户自己的接口额度。
- 插件包当前为 `unsigned`，仅用于开发者上传、预检、安装、调用和审计链路测试。

## 常见问题

### 没有可选择的 Workflow

先在 AI 资源中心创建并启用 Workflow 配置，确认凭据已配置、商品授权有效且配置未被吊销。

### 返回 20354 或参数 schema 错误

确认星辰 Workflow 开始节点的真实字段名是 `AGENT_USER_INPUT`。如果开始节点使用其他字段，应制作匹配该字段 key 的插件版本，不能只修改显示名称。

### 凭据不存在或权限不足

重新检查 AI 资源中心的凭据绑定，并确认安装时已经授权 Manifest 中声明的四项权限。

### 返回内容不是 Markdown

检查 Workflow 的结束节点输出和响应格式。插件会按 Markdown 渲染文本，但不会伪造或自动补全 Workflow 未返回的内容。

### 无法在开发者中心上传

当前开发者中心的商品版本上传仍要求旧市场 Manifest 和 `.zip`；本插件是 Manifest v3 `.firstwork-plugin`。请先在插件管理页完成真实安装与调用验收，市场发布桥接需由平台后续统一完成。
