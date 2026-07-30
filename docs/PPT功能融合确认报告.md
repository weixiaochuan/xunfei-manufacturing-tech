# PPT 功能融合确认报告

确认日期：2026-07-31

## 1. 对比范围

- 主体项目：`D:\ag\汇总\ag-collaboration-test`
- PPT 来源项目：`D:\ag\邹元杰融合审计\xunfei-manufacturing-tech\7.9 第一周\firstwork\Pomegranate`
- PPT 引擎资源来源：`D:\ag\邹元杰融合审计\xunfei-manufacturing-tech\7.9 第一周\firstwork\ppt-master`
- 执行规则：`AG多人协作与功能迁移说明.md`

## 2. 对比结果

主体项目的 PPT 实现已经覆盖来源项目，并包含更完整的长素材理解、上下文预算、分块合并、原生规划、密度、主题、质量和状态管理能力。

| 对比项 | 来源 | 主体 | 结论 |
|---|---:|---:|---|
| `src/pages/ppt-generation/index.tsx` | 2,084 行 | 2,597 行 | 主体实现更完整，不覆盖 |
| `src/store/pptGenerationDraft.ts` | 291 行 | 495 行 | 主体包含更完整的分块理解和状态机，不覆盖 |
| `src-tauri/src/services/ppt_master.rs` | 11,726 行 | 12,426 行 | 主体引擎更新，不覆盖 |
| `src-tauri/src/commands/ppt_master.rs` | 27 行 | 27 行 | SHA-256 一致，无需迁移 |
| `src/lib/ppt*` | 来源无对应完整模块 | 主体有 19 个实现与测试文件 | 主体更完整 |
| `ppt_master_native_*.rs` | 来源无独立子模块 | 主体有 5 个子模块 | 主体更完整 |

资源目录按相对路径比较：

- 主体有效资源文件：13,488 个。
- 来源有效资源文件：13,493 个。
- 来源多出的 5 个文件全部位于 `.github/`，为 Funding、Issue 模板和 Pages 部署工作流，不是 PPT 运行、生成或打包资源。
- 没有发现主体缺失的引擎脚本、模板、规则、视觉检查或导出资源。

## 3. 本轮决定

**不迁移 PPT 代码。**

原因：

1. 来源中没有发现主体缺失的功能文件或独立能力。
2. 主体的同名页面、Store 和 Rust service 都是更完整的实现。
3. 来源 command 与主体完全一致。
4. `ppt-master` 运行资源已经进入主体并在 Tauri 打包配置中注册。
5. 用来源整目录或旧同名文件覆盖主体，会丢失主体后续增加的能力。

本轮不创建空迁移 commit，不复制 `firstwork/Pomegranate`，不复制来源 `ppt-master` 覆盖主体资源。

## 4. 主体已有 PPT 能力

- PPT 页面、路由和 ActivityBar 入口。
- 手工素材、电脑文件、统一文档、日记和账号上传文件素材。
- AI 需求理解、长素材上下文预算、分块分析、并发控制、失败重试和分层合并。
- 可编辑的理解摘要、重点取舍、叙事主线、建议页面结构和视觉建议。
- 稳定模板生成与 `ppt-master` 原生实验模式。
- 内容密度、布局、主题、SVG 质量、原生兼容性、文本溢出和自动修复。
- 可编辑 PPTX 导出、输出目录选择、打开结果和定位目录。
- 内置 `src-tauri/resources/ppt-master` 引擎、脚本、规则、布局、图表和示例资源。

## 5. 后续开发入口

后续 PPT 功能开发应以主体实现为基线，严格增量进行：

- 页面：`src/pages/ppt-generation/`
- PPT 前端逻辑：`src/lib/ppt*`
- PPT 状态：`src/store/pptGenerationDraft.ts`
- Rust 业务：`src-tauri/src/services/ppt_master*.rs`
- Rust Command：`src-tauri/src/commands/ppt_master.rs`
- 随包资源：`src-tauri/resources/ppt-master/`

不应从 `firstwork` 重新开始或将旧同名文件替换主体。

