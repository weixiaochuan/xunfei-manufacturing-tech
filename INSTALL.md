# AG 协作开发安装说明

本文面向拿到 `ag` 压缩包的同伴，用于在本机恢复开发环境、建立独立分支，并按协作手册继续完善 AI 助学、AI 助研或 PPT 助手模块。

开始开发前必须先阅读：

- `AG多人协作与功能迁移说明.md`
- `AGENTS.md`
- `docs/account-classroom-isolation.md`
- `docs/summary3-cloud-local-archive.md`

## 1. 环境要求

推荐环境：

- Windows 10/11
- Node.js 22.x
- Corepack
- pnpm，由 Corepack 管理
- Rust stable
- MSVC C++ Build Tools
- WebView2 Runtime
- Git

Account Server 要求 Node.js `>=22 <23`。如果本机是 Node 24 或其他版本，可能出现 engine 警告或测试差异，建议切换到 Node 22。

Rust 工具链由 `rust-toolchain.toml` 指定：

```text
stable + x86_64-pc-windows-msvc
```

## 2. 解压位置

建议解压到不含特殊权限限制的路径，例如：

```text
D:\ag
```

如果多人在同一台电脑开发，不要共用同一个工作目录。每个人使用自己的目录或 Git worktree。

## 3. 首次检查

在项目根目录执行：

```powershell
git status -s
git branch --show-current
git rev-parse --show-toplevel
git rev-parse HEAD
node -v
corepack --version
rustc --version
cargo --version
```

如果 `git rev-parse HEAD` 失败，说明当前包没有有效提交基线。使用下面命令创建统一协作基线：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\init-collaboration-git.ps1
```

该脚本会使用固定作者、固定提交时间和固定提交说明创建基线提交。所有同伴必须基于同一份压缩包执行该脚本，后续分支才容易汇总。

## 4. 安装依赖

启用 Corepack：

```powershell
corepack enable
```

安装依赖：

```powershell
corepack pnpm install
```

如果网络访问 npm registry 失败，先确认代理或镜像配置。不要手工复制别人电脑上的 `node_modules/`。

## 5. 建立自己的功能分支

```powershell
git switch -c feature/learning-姓名-日期
```

或：

```powershell
git switch -c feature/research-姓名-日期
git switch -c feature/ppt-姓名-日期
```

禁止三个人在同一分支上开发。禁止直接在主分支、集成分支或别人分支上修改。

## 6. 前端开发

启动前端 Vite：

```powershell
corepack pnpm dev
```

默认地址：

```text
http://localhost:2010
```

`dev` 使用 `--strictPort`。如果 2010 端口已被占用，先确认是不是自己开的服务。不要直接 kill 端口，避免影响其他同伴。

## 7. Tauri 开发

启动桌面端：

```powershell
corepack pnpm tauri dev
```

当前 Tauri 配置会调用 `pnpm dev`，不会主动 kill 端口。

## 8. Account Server

Account Server 位于：

```text
services/account-server
```

它需要 `.env`，但源码包只提供 `.env.example`。复制并按主负责人提供的开发环境填写：

```powershell
Copy-Item services\account-server\.env.example services\account-server\.env
```

注意：

- `.env` 不允许提交。
- 数据库密码、Casdoor Secret、session token 不允许写进前端代码。
- `USER_FILES_ROOT` 必须指向源码目录外的运行时文件目录。

常用命令：

```powershell
corepack pnpm --dir services/account-server build
corepack pnpm --dir services/account-server migrate
corepack pnpm --dir services/account-server dev
corepack pnpm --dir services/account-server test
```

## 9. 构建和测试

前端构建：

```powershell
corepack pnpm build
```

Tauri 检查：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1
```

账号隔离相关测试：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1 -CargoArgs "test --manifest-path src-tauri\Cargo.toml account::tests --lib"
powershell -ExecutionPolicy Bypass -File scripts\check-tauri-with-e-sdk.ps1 -CargoArgs "test --manifest-path src-tauri\Cargo.toml services::data_dir --lib"
corepack pnpm --dir services/account-server test
```

## 10. 协作打包

主负责人生成给同伴的源码包时，使用：

```powershell
corepack pnpm package:collab
```

输出目录：

```text
collab-packages/
```

打包脚本会排除：

- `.git/`
- `node_modules/`
- `dist/`
- `target/`
- `.env`
- 日志
- 临时文件
- 运行时数据目录

## 11. 提交规则

只提交自己明确修改的文件：

```powershell
git add <具体文件1> <具体文件2>
git diff --cached --name-only
git commit -m "feat(助学): 接入学习项目诊断结果页面"
```

禁止：

- `git add -A`
- `git add .`
- `git stash`
- `git reset --hard`
- `git clean -fd`
- 删除不认识的文件
- 提交 `node_modules/`、`dist/`、`target/`、`.env`、数据库、日志、上传文件、用户数据

## 12. 开发完成交付

每位同伴交付：

- Git 分支名
- 提交列表
- `docs/模块名_迁移报告.md`
- 测试命令和结果
- 人工验收步骤
- 新增依赖清单
- 新增环境变量清单
- 新增本地数据目录清单
- 新增云端接口清单
- 数据库变更说明
- 未完成问题说明

不要只交一个改完后的完整文件夹。最终汇总需要可追踪的分支、提交和差异。
