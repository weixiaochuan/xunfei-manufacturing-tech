# firstwork 第一阶段整合交付说明（2026-07-12）

## 1\. 文档用途

本文档用于说明：

1.  `firstwork` 当前目录的真实结构；
    
2.  第一阶段整合后的启动与配置方式；
    
3.  当前已经完成的整合与修复内容；
    
4.  后续继续联调、验收、清理时需要注意的事项；
    
5.  数据库目录、版本和默认链路的配置方法。
    

本文档以：

```text
D:\ag\firstwork
```

为准，不再沿用旧说明中已经过时的历史路径写法。

* * *

## 2\. 当前项目定位

`firstwork` 是第一阶段整合后的最终工作目录，用于承接并继续推进：

1.  `Pomegranate` 主程序；
    
2.  AI 助学功能；
    
3.  AI PPT 生成功能；
    
4.  `ppt-master` 本地引擎接入；
    
5.  第一轮整合后的 Tauri 前后端命令链修复。
    

约束说明：

1.  后续开发只应修改 `D:\ag\firstwork`。
    
2.  `pomegranate-ai-ppt`、`zuxue`、`Zhuhai` 仅作为历史参考，不应再作为运行依赖。
    
3.  `firstwork` 应能独立运行，不依赖外部同级目录中的源码文件。
    

* * *

## 3\. 当前目录结构

建议按下面理解 `firstwork`：

```text
firstwork/
|-- Pomegranate/           # 主应用（Tauri + React + Rust）
|-- ppt-master/            # PPT 生成引擎
|-- learning-assistant/    # 助学资源 / Skill 相关内容
|-- 启动方法.md             # 旧版说明，部分路径和命令已过时
|-- ReadMe_7-12.md         # 本文档
`-- 使用心得.doc
```

运行后可能出现：

```text
.runtime-data/
.runtime-data-final/
.runtime-data-task-verify/
*.log
*.png
```

这些大多是运行数据库、验证截图和日志，不属于核心源码交付内容。

* * *

## 4\. 技术栈

### 4.1 主程序

`Pomegranate` 当前实际技术栈：

1.  React 19
    
2.  TypeScript
    
3.  Vite
    
4.  Tauri 2
    
5.  Rust / Cargo
    
6.  pnpm
    

### 4.2 PPT 引擎

`ppt-master` 当前实际技术栈：

1.  Python 3.11
    
2.  `pip + requirements.txt`
    
3.  独立 `.venv`
    

注意：

```text
ppt-master 根目录的 requirements.txt 继续引用 skills/ppt-master/requirements.txt
```

因此安装方式仍以：

```powershell
python -m pip install -r requirements.txt
```

为准。

* * *

## 5\. 环境要求

建议准备：

1.  Node.js 22 或更高
    
2.  pnpm
    
3.  Rust / Cargo
    
4.  Visual Studio C++ Build Tools
    
5.  Python 3.11
    
6.  Git
    
7.  WPS 或 Microsoft PowerPoint
    

基础检查命令：

```powershell
node -v
pnpm -v
rustc -V
cargo -V
python -V
```

* * *

## 6\. 依赖恢复方式

### 6.1 ppt-master

进入：

```powershell
cd D:\ag\firstwork\ppt-master
```

如 `.venv` 不存在：

```powershell
py -3.11 -m venv .venv
```

激活：

```powershell
.\.venv\Scripts\Activate.ps1
```

如 PowerShell 阻止执行：

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\.venv\Scripts\Activate.ps1
```

安装依赖：

```powershell
$env:PYTHONUTF8="1"
python -X utf8 -m pip install -r requirements.txt
python -X utf8 -m pip check
```

### 6.2 Pomegranate

进入：

```powershell
cd D:\ag\firstwork\Pomegranate
```

安装前端依赖：

```powershell
pnpm install
```

构建 MCP sidecar：

```powershell
pnpm build:mcp
```

如遇 `esbuild` 异常，可执行：

```powershell
pnpm approve-builds
pnpm rebuild esbuild
```

* * *

## 7\. 正确启动方式

### 7.1 启动方式1（隔离运行数据）

为避免默认数据库版本冲突，建议使用独立数据目录启动：

```powershell
$env:KB_DATA_DIR="D:\ag\firstwork\.runtime-data-final"
cd D:\ag\firstwork\Pomegranate
pnpm tauri:dev
```

说明：

1.  `KB_DATA_DIR` 用于把数据库、附件、插件状态和运行时资源隔离到 `firstwork` 内部；
    
2.  当前仓库实际脚本名是 `tauri:dev`；
    
3.  因此应优先使用 `pnpm tauri:dev`，而不是旧说明中的 `pnpm tauri dev`。
    

### 7.2 不隔离数据目录的启动方式

如明确要使用系统默认数据目录：

```powershell
cd D:\ag\firstwork\Pomegranate
pnpm tauri:dev
```

但这可能触发历史数据库版本冲突，例如：

```text
数据库版本(43)高于应用支持的版本(42)
```

因此不建议作为日常验证方式。

本地指针已修改，可以正常启动

### 7.3 PowerShell 注意事项

不要在旧版 PowerShell 中直接使用：

```powershell
cd D:\ag\firstwork\Pomegranate && pnpm tauri:dev
```

因为部分 PowerShell 版本不支持 `&&`。

推荐写法：

```powershell
cd D:\ag\firstwork\Pomegranate
pnpm tauri:dev
```

或者：

```powershell
cmd /c "cd /d D:\ag\firstwork\Pomegranate && pnpm tauri:dev"
```

* * *

## 8\. 数据库配置与说明

### 8.1 数据目录选择优先级

程序当前的数据目录选择顺序为：

1.  启动时显式传入的环境变量 `KB_DATA_DIR`
    
2.  本机框架数据目录中的指针文件 `data_dir.txt`
    
3.  框架默认应用数据目录
    

因此，最稳妥的方式始终是显式设置：

```powershell
$env:KB_DATA_DIR="D:\ag\firstwork\.runtime-data-final"
```

### 8.2 本机框架默认目录

当前调试版在 Windows 下的框架目录实际为：

```text
C:\Users\Yoj\AppData\Roaming\edu.bit.inb-dev
```

其中的数据目录指针文件为：

```text
C:\Users\Yoj\AppData\Roaming\edu.bit.inb-dev\data_dir.txt
```

这个文件只保存“当前默认运行数据目录”的路径。

### 8.3 firstwork 当前推荐数据库位置

当前建议统一使用 `firstwork` 内部独立运行目录：

```text
D:\ag\firstwork\.runtime-data-final
```

其中数据库相关文件通常为：

```text
D:\ag\firstwork\.runtime-data-final\dev-app.db
D:\ag\firstwork\.runtime-data-final\dev-app.db-wal
D:\ag\firstwork\.runtime-data-final\dev-app.db-shm
```

说明：

1.  `dev-app.db` 是调试运行使用的主数据库；
    
2.  `-wal` 与 `-shm` 是 SQLite 正常运行时附属文件；
    
3.  这些文件属于运行数据，不属于源码。
    

### 8.4 当前数据库版本状态

代码中的数据库 schema 版本当前为：

```text
SCHEMA_VERSION = 42
```

`firstwork` 当前独立运行链路已经验证可使用版本 `42` 正常启动。

### 8.5 为什么会出现“43 高于 42”

此前本机默认链路曾连接到其他目录中的旧运行数据库，例如：

```text
D:\ag\ap\pomegranate\dev-app.db
```

该数据库已经被其他版本程序迁移到 `43`。而当前 `firstwork` 中的程序只支持到 `42`，因此启动时会直接拒绝打开该库，并报出：

```text
数据库版本(43)高于应用支持的版本(42)，请升级应用
```

这不是 `firstwork` 源码损坏，而是“当前程序版本”和“被连接到的历史数据库版本”不一致导致的保护性报错。

### 8.6 当前默认链路说明

当前本机默认指针文件已经改为指向：

```text
D:\ag\firstwork\.runtime-data-final
```

因此：

1.  最稳妥的方式仍然是启动前显式设置 `KB_DATA_DIR`；
    
2.  即使未设置环境变量，本机当前默认链路也应优先落到 `firstwork` 自己的数据目录。
    

### 8.7 data\_dir.txt 注意事项

`data_dir.txt` 必须满足以下要求：

1.  文件内容只保留一行纯文本路径；
    
2.  不要加引号；
    
3.  不要带 UTF-8 BOM；
    
4.  不要混入空格或其他不可见字符。
    

如果该文件被错误写入 BOM 或非法字符，启动时可能出现类似：

```text
IO 错误：文件名、目录名或卷标语法不正确。(os error 123)
```

### 8.8 交付建议

1.  只交付 `firstwork` 源码时，其他机器不会天然继承你本机的历史数据库版本冲突；
    
2.  后续使用者首次启动时，仍建议通过 `KB_DATA_DIR` 指向项目内部独立目录；
    
3.  若需要演示稳定功能，可以保留 `.runtime-data-final` 作为本项目本地运行数据；
    
4.  若只交付源码而不交付运行数据，也可以由对方首次启动后自动生成新的数据库。
    

* * *

## 9\. 第一阶段整合后已确认完成的内容

1.  `zuxue` 与 `Zhuhai` 的有效源码修改已并入 `firstwork`
    
2.  AI 助学入口、PPT 生成入口、待办相关调用链已完成整合
    
3.  依赖安装、TypeScript 检查、前端构建、`cargo check`、`cargo test` 已通过
    
4.  Tauri 应用可以启动
    
5.  两类 Skill 已放在 `firstwork` 内部使用，不依赖外部目录
    
6.  已修复多组 Tauri IPC 映射问题，包括：
    
    -   `task`
        
    -   `ai_model`
        
    -   `config`
        
    -   `ppt_master`
        
    -   `system`
        
    -   `note`
        
    -   `folder`
        
    -   `daily`
        
    -   `tag`
        
    -   `prompt`
        
    -   `plugin`
        
    -   `hidden`
        
    -   `hidden_pin`
        
    -   `trash`
        

* * *

## 10\. 已做过的运行验证

已实际执行并确认过：

```powershell
cd D:\ag\firstwork\Pomegranate
pnpm build:mcp
pnpm exec tsc --noEmit
pnpm build
cargo check --manifest-path .\src-tauri\Cargo.toml
cargo test --manifest-path .\src-tauri\Cargo.toml
```

也已使用：

```powershell
$env:KB_DATA_DIR="D:\ag\firstwork\.runtime-data-final"
pnpm tauri:dev
```

完成以下方向验证：

1.  主界面可打开
    
2.  AI 助学页面可进入
    
3.  PPT 生成功能页面可进入
    
4.  待办页面基础调用链已恢复
    
5.  多组页面不再出现大面积 `Command xxx not found`
    

注意：

1.  AI 助学若涉及真实模型调用，仍需要配置可用 API；
    
2.  PPT 生成功能若涉及真实理解与生成，仍需要有效的 `ppt-master` 路径、Python 路径和模型配置。
    

* * *

## 11\. firstwork 中哪些内容是源码，哪些是运行产物

源码 / 必须保留：

```text
firstwork\Pomegranate
firstwork\ppt-master
firstwork\learning-assistant
firstwork\启动方法.md
firstwork\ReadMe_7-12.md
```

运行 / 验证产物：

```text
.runtime-data
.runtime-data-final
.runtime-data-task-verify
*.log
*.png
```

例如：

```text
.runtime-data-final\dev-app.db
.runtime-data-final\dev-app.db-wal
.runtime-data-final\dev-app.db-shm
```

这些都属于运行数据或验证产物，不是功能源码。

* * *

## 12\. 可以清理但通常不影响功能的内容

1.  `node_modules/`
    
2.  `target/`
    
3.  `.venv/`
    
4.  `dist/`
    
5.  `build/`
    
6.  `.pptx-build-*`
    
7.  `*.log`
    
8.  `exports/` 下的导出 PPT / PDF
    
9.  验证截图
    
10.  临时运行数据库目录
     

清理前提：

1.  你确认不再依赖这些文件做当前演示；
    
2.  你保留了源码、配置、锁文件和必要模板资源；
    
3.  你知道删除后下次运行可能需要重新安装依赖或重新生成数据库。
    

* * *

## 13\. 暂不建议清理的内容

1.  `package.json`
    
2.  `pnpm-lock.yaml`
    
3.  `Cargo.toml`
    
4.  `requirements.txt`
    
5.  `Pomegranate/src-tauri/resources/` 下的资源
    
6.  `learning-assistant/` 内的 Skill、提示词和助学资源
    
7.  `ppt-master/` 内实际被调用的模板、脚本和引擎代码
    

* * *

## 14\. 后续建议的人工终验顺序

1.  用独立数据目录启动 `Pomegranate`
    
2.  先验证 AI 助学页面的引擎检测、理解目标、学习计划链路
    
3.  再验证 PPT 生成页面与 `ppt-master` 的连接
    
4.  完成一次最小导出
    
5.  从 AI 助学切到 PPT，再切回 AI 助学，确认状态互不污染
    

* * *

## 15\. 如果后续再次启动失败，优先排查

1.  是否误连到了外部旧数据库
    
2.  `KB_DATA_DIR` 是否配置正确
    
3.  `data_dir.txt` 是否含 BOM 或非法字符
    
4.  `ppt-master\.venv\Scripts\python.exe` 是否仍存在
    
5.  `firstwork` 内部 Skill 路径是否仍有效