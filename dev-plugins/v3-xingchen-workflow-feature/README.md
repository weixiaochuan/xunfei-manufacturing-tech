# 星辰 Workflow 动态表单示例

此包不包含 API Key、API Secret、Token、Flow ID 或 Endpoint。使用前请先在 firstwork 的
AI 资源中心配置并启用自己的讯飞星辰 Workflow，然后在功能页选择该配置。

提交时，表单字段会原样构造成 Workflow `parameters`；Rust 后端负责读取安全凭据、
检查商品与插件权限并发起请求。真实调用会把填写内容发送到讯飞星辰，并可能消耗用户额度。
