# Pomegranate 插件开发指南

本目录保存团队制作 Pomegranate Manifest v3 受控插件时应遵循的通用流程。它不是某个插件的一次性交付说明，也不包含可执行插件代码或任何用户凭据。

## 阅读顺序

1. 先阅读 `Pomegranate插件规范制作流程.md`，确认插件分类、运行时、权限和目录结构。
2. 再查看上级目录的 `PLUGIN_V3_EXAMPLES.md` 与三个 `v3-*` 安全示例。
3. 制作星辰 Workflow feature 时，可参考 `../v3-document-summary/`。
4. 打包前再次核对当前项目的 `scripts/plugin-package.mjs`，因为脚本校验规则高于历史文档示例。

## 适用范围

- Manifest v3 `feature`、`enhancement`、`hybrid` 插件。
- `declarative-ui`、`prompt-pack`、`xingchen-agent`、`xingchen-workflow` 受控运行时。
- 本地开发目录调试和 `.firstwork-plugin` 安装包制作。
- 使用 AI 资源中心中已有 ExternalAgent 调用星辰 Workflow。

不适用于 legacy-js、任意脚本执行、真实市场支付、服务端签名签发或旧 Manifest v2 商品包。当前开发者中心的本地市场上传仍使用旧 v2 `.zip` 链，详见主流程文档的“文档与代码差异”。
