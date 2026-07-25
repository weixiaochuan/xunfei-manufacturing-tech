# 学-练-考助学支持模块 · Phase 1 MVP

> **firstwork 安全说明：** 本目录只用于题库生产、维护和教师分析，不是学生端运行目录。请先阅读 [README_FIRSTWORK.md](README_FIRSTWORK.md)。所有数据库命令必须通过 `qbctl.py --db <开发库绝对路径>` 执行，禁止把学生正式库作为导入、重建、seed 或批量审核目标。

> **本轮升级（第一根柱子·出题）：**
> 1. 知识库从 2 章扩到**全部 7 章，共 283 个知识点**（`db/init_db.py` 已收全）。
> 2. **多模型自选**：`config/llm_config.json` 改成"模型注册表"，可登记 DeepSeek/讯飞星火/Kimi/GLM 等任意多个，出题时用 `--provider 名字` 选，方便同一批知识点多模型对比。
> 3. **误区库自动积累**：每出一道概念题，其干扰项标注的认知误区会自动回写误区库（`source=llm_mined`），不用纯手工填。
> 4. **给助学的接口层** `integration/api.py` + `integration/接口说明.md`：助学"开始测试"按钮按合同调用即可取题，我们内部改动不影响它。
> 5. 新手上手看 **`运行指南_小白版.md`**（从装 Python 到接 DeepSeek 出题，逐步带）。
>
> 真实出题内容仍需你在自己电脑上接模型跑（沙箱无外网），当前库里是 mock 占位内容。


对应《基于知识库的学-练-考助学支持模块调研》第6.5节 Phase 1（MVP阶段）的可交付成果：
> 可用的题库（含各层次字段）+ 可出题 + 可答题 + 可看对错，人工审核通过率 > 80%

当前进度：**题库元数据结构 + 知识点导入 + 出题流水线（概念题/计算题分开）+ 质量校验 + 人工审核闭环** 已跑通。答题功能、Bloom配额规划、反馈生成等留到下一步（见文末"还没做的部分"）。

## 目录结构

```
question_bank_system/
├── data/                          知识库Excel原始数据（已含你提供的两章）
├── db/
│   ├── schema.sql                 数据库表结构（五层字段设计，对应报告6.2节）
│   ├── init_db.py                 建表 + 导入知识库Excel
│   ├── seed_misconceptions.py     认知误区库种子数据（报告2.4节(1)）
│   └── question_bank.db           SQLite数据库（运行过init_db.py后生成）
├── llm/
│   └── client.py                  模型无关的LLM客户端（换模型只改配置，不改代码）
├── config/
│   ├── llm_config.example.json    配置模板
│   └── llm_config.json            实际配置（默认mock模式，接真实API前需要改这个）
├── prompts/
│   ├── concept_question_prompt.py     概念题Prompt（融合秘塔/学科网/讯飞三个借鉴点）
│   └── computation_question_prompt.py 计算题Prompt（独立流水线，为验算做准备）
├── pipeline/
│   ├── generate_questions.py      出题主流程：知识点→Prompt→LLM→质检→入库
│   └── validators.py              质检逻辑（概念题字段校验 + 计算题算式自动验算）
└── review/
    ├── export_for_review.py       导出/回写人工审核结果
    └── pending_review.xlsx        当前待审核题目（跑过export后生成）
```

## 快速开始

```bash
pip install -r requirements.txt

# 1. 建库 + 导入知识库Excel（--reset 会清空重建）
python3 db/init_db.py --reset

# 2. 灌入认知误区种子数据
python3 db/seed_misconceptions.py

# 3. 出题（mock模式，不需要真实API Key，用来验证流程本身没问题）
python3 pipeline/generate_questions.py --chapter 第一章_绪论 --limit 10 --type auto
python3 pipeline/generate_questions.py --chapter 第二章_机械加工工艺规程设计 --limit 10 --type auto

# 4. 导出待审核题目到Excel，人工在review_status列打"已通过"/"已驳回"
python3 review/export_for_review.py export
# ... 打开 review/pending_review.xlsx 人工审核 ...
python3 review/export_for_review.py import_back
```

当前仓库里的 `db/question_bank.db` 已经是跑过一遍上述流程的状态（78条知识点、5条种子误区、15道demo题目、其中4道已走完人工审核回写），你可以直接打开看效果，想从头跑就加 `--reset`。

## 接入真实大模型（讯飞星火 / DeepSeek / 秘塔…都可以）

现在跑的是 `llm/client.py` 里的 `MockProvider`，只是把知识点标题拼进一个格式正确但内容是占位符的题目里，**用来验证"知识点→出题→质检→入库"这条工程链路本身没问题**，题目内容本身不能用。

接真实模型三步：
1. 打开 `config/llm_config.json`
2. 把 `concept` / `computation` 里的 `"provider": "mock"` 改成：
   ```json
   {
     "provider": "openai_compatible",
     "base_url": "讯飞星火或DeepSeek的接口地址",
     "api_key": "你的Key",
     "model": "具体模型名"
   }
   ```
3. 重新跑 `pipeline/generate_questions.py`，不需要改任何业务代码。

概念题和计算题的模型可以配成不一样的（比如概念题用讯飞星火、计算题按报告6.4节结论换DeepSeek），因为两条流水线本来就是分开跑的。

> 沙箱环境本身访问不了外部大模型API（网络白名单限制），所以这一步需要你在自己的开发环境里跑，不是这个演示环境的问题。

## 已经内置的几个报告结论

- **一题一验证的计算题流水线**（报告2.4节/4.2节）：`validators.py` 里的 `validate_computation_question` 会把模型返回的 `calculation_steps` 里的算式抠出来重新算一遍，跟模型自己给的结果比对，算不对的直接标记"验算失败"打回。**这是最需要你们盯着优化的地方**——现在只能识别 `3.5 + 0.8 - 1 = 3.3` 这种规整算式，公式更复杂或者模型用文字描述而不写算式时会退化成"无法自动验算-需人工复核"，是诚实的能力边界，不是bug，可以随着实际生成样本逐步把正则换成更严格的解析器。
- **干扰项对应认知误区**（学科网逻辑，报告2.4节）：`misconceptions` 表 + `concept_question_prompt.py` 里强制要求每个干扰项标注对应的认知误区，质检时会检查这个字段是否为空。
- **来源可追溯**（秘塔逻辑）：每道题的 `source_node_id` 外键关联回 `knowledge_points`，答错后可以直接跳回原文（反馈模块要用）。
- **模型无关**：确认了比赛不强制用讯飞星火后，客户端做成了任意OpenAI兼容接口都能接，不锁定单一厂商。

## 还没做的部分（对应报告Phase 2 / Phase 3，不在这次MVP范围内）

- 学生答题界面 + 任务层反馈（对错判定）—— `student_answers` 表已建好，等你们定好前端/交互形式后接入
- 出题配额规划（按知识点×Bloom层级×题型均衡分配，报告6.3.1步骤2）—— 目前是"给哪个知识点就出哪个"，还没做整章覆盖度的自动配额
- 过程层/自我调节层反馈生成（报告5.4节）—— 需要先有答题数据才能触发，`student_answers.feedback_json` 字段已预留
- IRT参数标定、BKT/DKT知识追踪、CAT选题 —— 报告6.5节Phase 3的内容，需要真实答题数据积累到一定量级才能开始
- 教师端班级统计看板 —— Phase 2

建议按报告6.5节的顺序往下推，先把"真实模型接入 + 出题质量到能用" 这一步做扎实，再往答题/反馈方向扩。
