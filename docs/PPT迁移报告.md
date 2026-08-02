# PPT 正式功能迁移报告

日期：2026-08-02

## 1. 迁移范围

- 负责模块：PPT 助手
- 主体项目：`D:\ag\汇总\ag-collaboration-test`
- 增强参考项目：`D:\大学\大二上各种作业\大三下\科大飞讯\xunfei-manufacturing-tech\7.9 第一周\Zhuhai\Pomegranate\Pomegranate`
- 参考项目分支：`cloud-deployment-prep-20260723`
- 参考项目 HEAD：`91688318beff184266b67ae3c5b4484463591581`
- 主体基线分支：`baseline/ag-collaboration`
- 起始 HEAD：`e3b54266e07a37876ae4f2dcb3ca32ec9dce2254`
- 功能分支：`feature/ppt-integration-20260802`
- 执行规范：`AG多人协作与功能迁移说明.md`

## 2. 增量融合判断

主体已有 PPT 页面、store、前端 lib、Tauri command、基础 Rust service、`ppt-master` 资源及打包配置。本轮没有复制旧工程，也没有替换主体页面。

只迁移参考版本中主体确实缺失的能力：

- 严格原生 PPT 生成流程。
- 设计系统契约和页面级视觉分配。
- 视觉细节检查与处理。
- 更完整的文本几何检查和安全修复。
- 生成规划、状态恢复和主题能力增强。
- 对主体既有 AI service 的兼容桥接。

主体原有模板/稳定流水线、质量检查、长材料理解和前端交互全部保留。

## 3. 新增文件

- `src-tauri/src/services/ppt_master_strict.rs`
- `src-tauri/src/services/ppt_master_native_design_system.rs`
- `src-tauri/src/services/ppt_master_native_visual_details.rs`
- `src-tauri/scripts/ppt_native_visual_details.py`

## 4. 修改文件

- `src-tauri/src/services/ppt_master.rs`
- `src-tauri/src/services/ppt_master_native_density.rs`
- `src-tauri/src/services/ppt_master_native_planning.rs`
- `src-tauri/src/services/ppt_master_native_state.rs`
- `src-tauri/src/services/ppt_master_native_theme.rs`
- `src-tauri/scripts/ppt_native_text_geometry.py`
- `docs/PPT迁移报告.md`
- `docs/PPT融合验收报告.md`

没有删除文件。

## 5. 公共和受保护区域

本轮没有修改：

- React PPT 页面、`pptGenerationDraft` 和 `src/lib/ppt*`。
- `src/Router.tsx`、`ActivityBar.tsx`、`AppLayout.tsx`。
- `src-tauri/src/lib.rs`、`commands/mod.rs`、`services/mod.rs`。
- `src-tauri/tauri.conf.json`、`package.json`、`Cargo.toml` 和锁文件。
- `src-tauri/src/account*`、`services/account-server/*`、Session、Deep Link。
- SQLite schema、Cloud 配置、助学和助研主体逻辑。

因此无需修改公共入口；主体原有 PPT 路由、导航和打包配置已经满足正式能力接入。

## 6. 依赖、配置和数据边界

- 未新增依赖。
- 未新增 Secret、数据库或云端接口。
- 新增可选本地回退开关：`POME_PPT_NATIVE_ENGINE=baseline`，仅用于对比主体原有原生引擎。
- PPTX、临时 SVG/图片和生成项目继续保存在用户本地，不进入源码、Account Server 或账号数据。
- 未将参考项目的 `.venv`、缓存、`projects` 或生成文件迁入主体。

## 7. 验证结果

- `pnpm build`：PASS，9182 个模块完成转换。
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`：PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml services::ppt_master --lib`：PASS；174 通过、0 失败、6 忽略。
- 两个新增/更新 Python 脚本语法检查：PASS。
- PPT 页面、路由、ActivityBar 入口：存在。
- `src-tauri/resources/ppt-master`：存在。
- `tauri.conf.json` 的 PPT resource 配置：存在。
- 受保护文件差异：0。

Rust check 产生 310 条 warning，主要为主体既有未使用代码以及严格引擎中尚未从所有入口调用的辅助函数，不阻止构建。6 个忽略测试依赖真实 AI 或本地验收环境，未伪造通过。

## 8. 人工验收项

本轮未启动 Tauri GUI，以下仍需人工确认：

- 点击 PPT 入口进入页面。
- 使用已配置模型完成一次真实生成。
- 检查导出 PPTX 和临时项目只落在用户本地。
- 检查生成质量和页面可编辑性。

## 9. 未迁移内容

- 参考项目旧页面、旧 store、旧前端 lib 和公共入口。
- 参考项目完整 `ppt-master` 工作目录。
- 任何账号、云端、数据库、缓存和用户生成数据。

## 10. 汇总注意事项

后续应以该功能分支的主体 PPT 实现为唯一基线继续增量开发。若与其他模块发生公共文件冲突，保留主体账号、路由和打包配置，仅审查 PPT 模块内部变更。
