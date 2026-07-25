# firstwork 题库生产与教师工具

本目录是从 `liu` 整理出的独立题库生产体系。它服务于导入、生成、审核、批改和教师分析，**不属于 Pomegranate 学生端源码，也不会自动修改学生正式题库**。

## 数据边界

- 生产工具与原始资料：`D:\ag\firstwork\question-bank-tooling`
- 开发/验证数据库：建议放在 `question-bank-tooling\.data\`，必须通过命令显式指定绝对路径。
- 学生端正式库：`D:\ag\firstwork\files.v21_最终\question_bank_system\db\question_bank.db`
- 学生端运行筛选：仅 `review_status='已通过'`、`usage_scope='学生练习'`、有标准答案且无 `no_answer_reason` 的题目。

`qb_runtime.py` 会拒绝源码目录默认库和学生正式库。写命令会在首次写入前把已有开发库备份到同目录 `backups/`。不要把 `.data`、备份、RAG 索引或本地模型配置提交到版本库。

## 环境

```powershell
cd D:\ag\firstwork\question-bank-tooling
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

真实模型配置只可从 `config/llm_config.example.json` 复制为本地忽略的 `config/llm_config.json`。不要把 API Key 写入源码、数据库、日志或 `.env`。未配置时默认角色使用 `mock`，不会联网。

Word 中 WMF/EMF 公式转图还需要本机 LibreOffice；没有它时文本与普通图片仍可处理，但公式转换属于 PARTIAL。

## 创建安全开发库

初始化脚本拒绝覆盖已有目标：

```powershell
.\.venv\Scripts\python.exe tools\create_dev_database.py `
  --db D:\ag\firstwork\question-bank-tooling\.data\development.db
```

所有后续命令都必须通过 `qbctl.py --db <绝对路径>`：

```powershell
$db = 'D:\ag\firstwork\question-bank-tooling\.data\development.db'
.\.venv\Scripts\python.exe qbctl.py --db $db status
.\.venv\Scripts\python.exe qbctl.py --db $db import-word --scan
.\.venv\Scripts\python.exe qbctl.py --db $db import-exercise --scan
.\.venv\Scripts\python.exe qbctl.py --db $db import-exercise --apply
.\.venv\Scripts\python.exe qbctl.py --db $db generate --provider mock --limit 3 --no-rag
.\.venv\Scripts\python.exe qbctl.py --db $db rag --build
.\.venv\Scripts\python.exe qbctl.py --db $db review-export
.\.venv\Scripts\python.exe qbctl.py --db $db review-import path\to\reviewed.xlsx
.\.venv\Scripts\python.exe qbctl.py --db $db serve
```

可用入口还包括 `import-pdf`、`import-word-llm`、`auto-review`、`dedup`、`normalize`、`calibrate`、`coverage` 和 `check-web`。先运行对应命令的 `--help`，对写操作先复制开发库再执行。

## 目录说明

- `data/real_exams/`：Word、PDF 历年真题和教材习题原始资料。
- `assets/images/`：题目图片、公式、零件图和答案截图。
- `pipeline/`：导入、模型出题、RAG、去重、难度和覆盖率处理。
- `review/`：自动审核及 Excel 人工审核导入导出。
- `feedback/`：客观/主观题反馈、采分点、错题、掌握度和自适应推荐。
- `demo/`：独立 HTTP 学生练习演示和教师看板。
- `integration/`：供其他模块调用的题库合同；学生列表不返回答案或解析。
- `prompts/`、`llm/`：五类题型提示词和 OpenAI 兼容多模型客户端。

## 审核与发布

生产工具永远不直接把待审核数据推给学生。推荐流程：

1. 从已知版本创建独立开发库或副本。
2. 导入/生成题目，执行 RAG、去重、难度与覆盖率检查。
3. 自动审核后导出 Excel，由教师人工复核，再导回开发库。
4. 在副本上运行 HTTP 和学生合同测试。
5. 对拟发布库执行只读预检：

```powershell
.\.venv\Scripts\python.exe tools\publish_student_bank.py --source $db
```

6. 只有预检通过且人工确认后，才显式发布：

```powershell
.\.venv\Scripts\python.exe tools\publish_student_bank.py --source $db `
  --apply --confirm PUBLISH_REVIEWED_STUDENT_BANK
```

发布器只接受 firstwork 的固定正式库目标；它会先备份旧库、使用 SQLite backup + 原子替换发布，并同步已引用媒体。本次整合与测试没有执行正式发布。

## 测试

```powershell
$testDb = 'D:\ag\firstwork\question-bank-tooling\.test-data\placeholder.db'
.\.venv\Scripts\python.exe qbctl.py --db $testDb test
.\.venv\Scripts\python.exe qbctl.py --db $db check-web
```

## 已知 PARTIAL

- 真实多模型出题、自动审核和 LLM 主观题批改需要用户自行提供合规模型配置，本次仅验证 Mock/离线路径。
- WMF/EMF 公式转换依赖 LibreOffice，未纳入 Python 依赖安装。
- 独立 HTTP 服务适合本地教师工具与联调，尚未加入生产级身份认证、TLS、限流和部署体系。
- 教师看板已存在于独立服务；本阶段没有将其塞入普通学生导航。
