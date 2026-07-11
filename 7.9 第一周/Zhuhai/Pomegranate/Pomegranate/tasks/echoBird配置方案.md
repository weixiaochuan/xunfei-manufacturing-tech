# EchoBird 模型中心配置方案

## 决策背景

参考 EchoBird 的 Model Nexus 设计，将当前 W-NoteBook 的"AI模型"设置页升级为 **AI 模型中心 + 应用管理** 双 Tab 结构，实现类似 EchoBird 的模型中心体验。

## 技术约束

- 后端: Rust + Tauri 2.x
- 前端: React 19 + TypeScript 5.8 + Ant Design 5 + TailwindCSS 4
- 平台: Windows / macOS / Linux

## 总体架构

```
设置
└── AI 能力
    ├── Tab 1: AI 模型中心
    │   ├── 我的模型（Card Grid + 表格视图）
    │   ├── 模型目录（国内/国外/中转站本地）
    │   ├── 模型测试控制台（Phase 2 暂缓）
    │   └── AI Provider 注册表
    │
    └── Tab 2: 应用管理
        ├── AI 应用（Claude Code / Desktop / Cursor 卡片）
        ├── MCP 管理（内置 MCP + 外部 Server + 工具列表）
        └── Agent Runner（占位入口）
```

## 分阶段计划

### Phase 1: 页面重组，不改后端 ✅ 已完成

- [x] 设置页主 Tab `AI模型` → `AI能力`
- [x] `AI能力` 内二级 Tab：`AI 模型中心` / `应用管理`
- [x] MCP 设置迁移到应用管理
- [x] 功能不回退

### Phase 2: EchoBird 风格模型中心 ✅ 已完成

- [x] 模型列表改为 Card Grid + 表格视图
- [x] 右侧模型目录（国内服务商 / 国外服务商 / 中转站本地）
- [x] 一键填入表单
- [x] 模型图标（从本地 EchoBird 复制 SVG）
- [x] Provider 注册表移到模型中心下方
- [x] 新增千问、豆包、Minimax provider 预设
- [ ] 测试控制台（Phase 2 暂缓）

### Phase 3: 应用管理增强 📍 当前

- [ ] Claude Code / Claude Desktop / Cursor 卡片化
- [ ] MCP 安装状态统一展示
- [ ] MCP 工具列表分类展示
- [ ] Agent Runner 占位入口

### Phase 4: 后端能力升级

- [ ] 模型表增加 protocol / capabilities 字段
- [ ] API Key 三态更新
- [ ] Agent Runner 后端实现

## 关键决策

| 决策 | 结论 |
|------|------|
| Claude Code 是否作为聊天模型 | 否 — 作为执行引擎，在应用管理中管理 |
| MCP 归属 | 应用管理 — 不与模型配置混在一起 |
| 模型目录分类 | 国内服务商 / 国外服务商 / 中转站本地 |
| 图标来源 | 本地 EchoBird 项目复制 |
| 页面宽度 | max-width: 960px, margin: 0 auto 居中 |
