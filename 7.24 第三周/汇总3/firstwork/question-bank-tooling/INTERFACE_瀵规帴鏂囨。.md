# 出题与反馈模块 · 对接文档（给助学端 / 助教端 / UI 组）

> **这份文档是写给 AI 读的**（对方会让 GPT/Claude 读这份文档来对接）。
> 所以我把所有细节都写全了，包括字段含义、边界情况、坑。
> **如果对接时发现有任何一点没说清楚，那是这份文档的失职。**

**模块名**：学-练-考 出题与反馈引擎
**负责人**：出题组
**课程**：机械制造工艺学 / 设计与制造基础Ⅲ（北京理工大学，课程编号 100036359 / 100036371）
**最后更新**：v19

---

# 第一部分：这个文件夹里有什么

## 1.1 目录结构

```
question_bank_system/
├── db/
│   ├── question_bank.db          ★ 唯一的数据文件。所有东西都在这里：
│   │                               题目、知识点、学生作答记录、知识点掌握度
│   │                               (SQLite，直接拷走就能用)
│   └── rag_index.pkl             RAG 检索索引（可以删，跑一下 rag.py --build 就重建）
│
├── demo/
│   ├── serve_quiz.py             ★ HTTP 服务（所有 API 都在这里）
│   │                               启动：python demo/serve_quiz.py --port 8000
│   ├── quiz_page.html            学生端页面（可以整个替换成你们的 UI）
│   └── teacher_dashboard.html    教师端看板页面
│
├── pipeline/                     数据处理脚本（对接时用不到，但要知道数据是怎么来的）
│   ├── generate_questions.py     出题（调 DeepSeek，按知识点批量生成）
│   ├── import_real_exams.py      导入历年真题（.docx）
│   ├── import_exercise_pdf.py    导入教材习题（PDF，题目和答案按题号配对）
│   ├── import_pdf_exams.py       导入真题 PDF（把题目和答案分别截图）
│   ├── extract_formulas.py       把 Word 里的公式/零件图救出来（WMF → PNG）
│   ├── recommend.py              自适应推题（IRT 难度模型）
│   ├── calibrate_difficulty.py   难度标定
│   ├── rag.py                    ⭐ RAG 检索引擎（出题前先查资料）
│   ├── dedup.py                  去重（每次出完题要跑）
│   ├── enrich_real_questions.py  给真题补知识点和 Bloom 标签
│   └── check_web.py              发布前自检
│
├── feedback/
│   └── feedback.py               ★ 批改逻辑（采分点判分、错因分析、趁热打铁）
│
├── prompts/                      所有 Prompt 模板
│   ├── concept_question_prompt.py       单选题
│   ├── multichoice_question_prompt.py   多选题
│   ├── computation_question_prompt.py   计算题
│   ├── subjective_question_prompt.py    名词解释 / 简述题
│   └── feedback_prompt.py               批改
│
├── review/
│   └── auto_review.py            模型审核（出完的题自动过一遍，不合格的驳回）
│
├── knowledge_base/               7 章知识库（Excel 源文件）
│                                 ★ 知识图谱组可以直接用这个，不用重复建
│
├── assets/images/                题目配图（PNG）
│   ├── real_exams/               从真题 Word 里提取的零件图、公式
│   ├── exercise/                 从教材 PDF 里提取的插图
│   └── pdf_exams/                真题原卷截图（q开头=题干图，a开头=答案图）
│
└── data/real_exams/              原始真题文件（docx / PDF）
```

## 1.2 一句话说清数据流

```
知识库(Excel) ──→ pipeline/*.py ──→ db/question_bank.db ──→ demo/serve_quiz.py ──→ HTTP API
   (7章知识点)      (出题/导题/标定)      (唯一数据源)          (对外服务)         (你们调这个)
```

**你们只需要关心最后一步：HTTP API。** 前面的都是我们内部生产数据用的。

---

# 第二部分：接口

## 2.0 启动方式

```bash
cd question_bank_system
python demo/serve_quiz.py --port 8000            # 离线模式（不调大模型，批改是预存的解析）
python demo/serve_quiz.py --port 8000 --provider deepseek   # 在线模式（调 DeepSeek 做智能批改）
```

- **已开启 CORS**（`Access-Control-Allow-Origin: *`），你们的前端可以跨域直接调
- **已支持 OPTIONS 预检**，浏览器发 POST 之前的预检请求会正常响应
- 所有响应都是 `application/json; charset=utf-8`

## 2.1 ⚠️ 最重要的约定：student_id

**我们不做登录，不存学生账号。** 学生身份由你们（助学端）传给我们。

**传法有三种，任选一种**：
1. POST 请求体里放 `"student_id": "20231234"`
2. URL 参数 `?student_id=20231234`
3. 请求头 `X-Student-Id: 20231234`

**不传会怎样**：会落到 `demo_student` 这个默认账号上，**所有人的数据会混在一起**
（张三做的题会算到李四头上，掌握度、推题全乱）。**所以务必传。**

**student_id 用什么值**：你们定，我们只当字符串存。建议用学号。

---

## 2.2 `GET /api/questions` —— 取题

**用途**：拉取所有可练的题目，学生端渲染题目列表。

**参数**：无（返回全部，你们在前端按 `chapter` / `type` / `src` 筛选）

**返回**：题目数组

```json
[
  {
    "id": "Q_681e26cfd9",
    "chapter": "第一章_绪论",
    "chapter_no": 1,
    "node": "KN_CH1_001",
    "type": "单选",
    "stem": "根据知识点原文，机械制造技术最初的主要加工方式是什么？",
    "options": ["在机床上用切削方法加工", "用电火花进行加工", "用激光进行加工", "用化学腐蚀进行加工"],
    "bloom": "记忆",
    "total_score": null,
    "image": null,
    "is_real": false,
    "src": "AI生成",
    "scan": false,
    "model": "deepseek/deepseek-chat"
  }
]
```

**字段说明（逐个）**：

| 字段 | 类型 | 含义 | 注意 |
|---|---|---|---|
| `id` | string | 题目唯一 ID | 提交作答时要原样传回来 |
| `chapter` | string | 章节名 | 7 个值之一，见 §3.2 |
| `chapter_no` | int | 章节序号 1~7 | 方便排序 |
| `node` | string | 知识点 ID | 如 `KN_CH1_001`，对应 knowledge_points 表 |
| `type` | string | 题型 | **只有 5 种**：`单选` `多选` `名词解释` `简述` `计算` |
| `stem` | string | 题干 | |
| `options` | array\|null | 选项文本数组 | **只有 `单选`/`多选` 有**，其它题型是 `null` |
| `bloom` | string | 认知层级 | `记忆`/`理解`/`应用`/`分析` |
| `total_score` | int\|null | 本题满分 | 主观题有，选择题可能为 null（按 2 分算） |
| `image` | string\|null | 配图 URL | 如 `/assets/images/pdf_exams/q171e.png`，直接拼到域名后面用 |
| `is_real` | bool | 是不是历年真题 | |
| `src` | string | 题目来源 | `真题` / `教材习题` / `AI生成` |
| `scan` | bool | **是不是"原卷截图题"** | 见下方说明 ⚠️ |

### ⚠️ 关于 `scan: true` 的题（原卷截图题）

**这类题（目前 7 道，都是计算题）需要特殊处理：**

- 它们的 `stem` **文字是残缺的**（PDF 提取会把上标下标打散，`A₁` 变成三行）
- **真正完整的题目在 `image` 那张图里**（含零件图、尺寸链、公差符号）
- **前端必须提示学生"以图为准"**，否则学生对着乱码文字会懵

建议的前端提示文案：
> 📄 这是历年真题原卷。公式、尺寸链、零件图都在下面的图里，**以图为准**（上面的文字是自动提取的，可能有错行）。

### ⚠️⚠️ 极其重要：这个接口**不返回任何答案**

`answer` / `explanation` / `answer_image` 这些字段**统统不在这个接口里**。

**原因**：这个接口的返回会直接进浏览器，学生按 F12 就能看到。
如果把答案发过来，等于直接泄题。
（这是我们踩过的坑：早期版本真的把答案发在这里了。）

**答案只能从 `POST /api/answer` 拿，也就是学生提交之后。**

---

## 2.3 `POST /api/answer` —— 提交作答，拿批改结果

**用途**：学生提交答案 → 返回判分、标准答案、错因分析、同类题推荐。

**请求**：
```json
{
  "question_id": "Q_681e26cfd9",
  "answer": "学生的答案文本",
  "student_id": "20231234",
  "mode": "练习"
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `question_id` | ✅ | 从 `/api/questions` 拿到的 `id` |
| `answer` | ✅ | 学生答案。**格式见下方**⚠️ |
| `student_id` | 强烈建议 | 不传会混数据 |
| `mode` | 否 | `练习`(默认) / `测试`。测试模式不给即时反馈 |

### ⚠️ `answer` 字段的格式（按题型不同）

| 题型 | 传什么 | 例子 |
|---|---|---|
| `单选` | **选项的文本**（不是 A/B/C/D！） | `"在机床上用切削方法加工"` |
| `多选` | **选项文本，用 `；` 分隔** | `"自动化；柔性化；集成化"` |
| `名词解释` | 学生写的文字 | `"工序是指一个工人在一个工作地..."` |
| `简述` | 学生写的文字 | |
| `计算` | 学生写的解题过程 | |

> **注意**：单选/多选传的是**选项文本**，不是选项字母。
> 分隔符用中文分号 `；`（也兼容 `;` `,` `，` `|`）。

**返回（选择题）**：
```json
{
  "question_id": "Q_681e26cfd9",
  "question_type": "单选",
  "is_correct": false,
  "your_answer": "测试",
  "correct_answer": "在机床上用切削方法加工",
  "explanation": "机械制造技术最初是指用机械来加工零件的技术...",
  "review_knowledge_id": "KN_CH1_001",
  "knowledge_title": "机械制造技术的发展",
  "feedback_source": "offline",
  "reinforce": [ {"question_id": "...", "type": "简述", "stem": "...", "why": "这个概念再从别的角度练两道"} ]
}
```

**返回（多选题，多了判分字段）**：
```json
{
  "question_type": "多选",
  "is_correct": false,
  "score": 2.0,
  "total_score": 4,
  "grade_note": "答对一部分（没有答错的），得一半分",
  "correct_answer": "自动化；柔性化；集成化"
}
```

**多选题判分规则（老师定的，已实现）**：
- 全部答对 → **满分**
- 答对一部分，**且没有选错的** → **一半分**
- **但凡选错一个** → **0 分**

**返回（主观题 / 计算题）**：
```json
{
  "question_type": "计算",
  "reference_answer": "（1）依据：工艺能力指数公式 Cp = T/(6σ)。\n（2）代入：...",
  "reference_steps": ["第1步：...", "第2步：..."],
  "rubric": [ {"point": "写出工艺能力指数公式", "score": 3}, {"point": "代入计算得 Cp=0.833", "score": 3} ],
  "total_score": 10,
  "explanation": "解题思路：为什么用这个公式...",
  "answer_image": "/assets/images/pdf_exams/a171e111a5b.png",
  "score": 7.5,
  "step_score": 45,
  "result_score": 30,
  "process_feedback": "你第二步代入的时候公差带算错了..."
}
```

| 字段 | 含义 |
|---|---|
| `reference_answer` | **标准答案**（考生要在卷子上写的全部内容：依据+公式+代入算式+结论） |
| `rubric` | **采分点数组**，每条含 `point`(得分点) 和 `score`(分值)。这是老师原卷的评分标准 |
| `answer_image` | ⚠️ **原卷截图题的标准答案图**（含尺寸链图、公式、采分点）。`scan:true` 的题才有 |
| `score` | 得分（开启 LLM 时才有智能判分；离线模式是 null，让学生对照采分点自评） |
| `process_feedback` | 错因分析（只在答错时给，需要开启 LLM） |
| `reinforce` | **趁热打铁**：2~3 道同类题，见 §2.4 |

### ⚠️ `answer_image` 的用法

对于 `scan: true` 的题，**标准答案是一张图**（因为公式和尺寸链没法用文字表达）。
**前端必须在学生提交之后，把这张图显示在"标准答案"区域。**

这是我们踩过的坑：早期版本把题目和答案截在**同一张图**里贴在题干下面，
结果**学生还没提交就看到答案了**，这道题就废了。现在题干图和答案图是**分开的两张**。

---

## 2.4 `reinforce` 字段（趁热打铁）

答错之后自动带出来的 2~3 道同类题。

```json
"reinforce": [
  {
    "question_id": "X_5d2a35c58a",
    "type": "简述",
    "stem": "什么是机械加工工艺规程的设计原则？...",
    "why": "这个概念再从别的角度练两道"
  }
]
```

**推题逻辑**（我们自己实现的，你们不用管，但可以了解）：
系统先判断学生是**"概念没懂"**还是**"这个题型不行"**：
- 概念没懂（这个知识点正确率低）→ 推**同知识点、换个题型**的题
- 题型不行（这个题型整体正确率低）→ 推**同题型、别的知识点**的题
- **计算题只推计算题**
- **找不到同类题就返回空数组** —— 宁可不推，不拿不相干的题凑数

`why` 字段是给学生看的解释文案，可以直接显示。

---

## 2.5 `GET /api/recommend` —— 自适应推题

**用途**：根据学生的掌握度，推"跳一跳够得着"的题。

**参数**：
| 参数 | 默认 | 说明 |
|---|---|---|
| `student_id` | demo_student | ⚠️ 必传 |
| `n` | 5 | 推几道 |
| `goal` | 攻破薄弱 | 学习目标，3 选 1（见下） |
| `qtype` | 无 | 限定题型，如 `计算` |

**三个学习目标**：
| goal 值 | 含义 | 推的题的预计答对率 |
|---|---|---|
| `巩固基础` | 推有把握的题，建立信心 | 70~85% |
| `攻破薄弱` | 推跳一跳够得着的 | 50~70% |
| `提升拔高` | 推难题 | 30~50% |

**示例**：`GET /api/recommend?student_id=20231234&n=3&goal=攻破薄弱&qtype=计算`

**返回**：
```json
{
  "items": [ {"question_id": "...", "stem": "...", "type": "计算", "tag": "适中·薄弱点"} ],
  "goal": "攻破薄弱",
  "goal_desc": "推跳一跳够得着的题"
}
```

**⚠️ 当前的局限（必须知道）**：
题目难度值现在是**"冷启动估计值"**（根据题型和分值估的），**不是真实数据**。
需要**每道题至少 20 条真实作答记录**才能校准成真难度。
**在拿到真实学生数据之前，推题的准确性是有限的。** 这是我们最大的瓶颈。

---

## 2.6 `POST /api/ask` —— 随时提问

**用途**：学生对某道题有疑问，直接问。模型会**根据学生的具体作答**来讲，而不是复述标准答案。

```json
{ "question_id": "Q_xxx", "message": "为什么这里要用极值法而不是概率法？", "was_correct": false }
```

**⚠️ 需要开启 LLM**（`--provider deepseek`），离线模式下这个接口不可用。

---

## 2.7 `GET /api/teacher` —— 教师看板

**用途**：给助教端 / 教师端用。

**参数**：`chapter`（可选，按章节筛选）、`student`（可选，看单个学生）

**返回**：班级错误率、最难的知识点、学生认知误区排行、掌握度分布。

## 2.8 `GET /api/exam_pool` —— 教师出题素材库

**用途**：历年真题里**原卷没给标准答案**的题（35 道）。

**⚠️ 这些题绝对不能给学生做**（没有标准答案 → 没法给出正确反馈 → 错的反馈比没有反馈更糟）。
**只给老师出题参考用。** 每道都标注了出处（哪一年、哪张卷子、课程编号）。

返回里有 `usage_scope` 字段：`学生练习` / `教师出题`，请据此过滤。

## 2.9 `GET /api/wrong_book` —— 错题本 ⭐ v20 新增

**用途**：学生做错的题，按知识点归堆。

**参数**：`student_id`（⚠️ 必传）；`all=1`（可选，连"已攻克"的也返回）

**返回**：
```json
{
  "student_id": "20231234",
  "todo_count": 12,
  "mastered_count": 5,
  "weak_points": [
    {"knowledge": "工艺尺寸链的基本概念", "wrong_count": 4,
     "question_ids": ["Q_xxx", "Q_yyy"]}
  ],
  "todo": [
    {"question_id": "Q_xxx", "type": "计算", "stem": "...", "src": "真题",
     "your_answer": "学生上次写的", "attempts": 2, "status": "待攻克",
     "knowledge_title": "工艺尺寸链的基本概念", "image": null}
  ]
}
```

**"攻克"的判定逻辑**：看这道题**最近一次**作答——
- 最近一次做对了 → `已攻克`，自动移出 `todo`
- 最近一次还是错 → `待攻克`，留在 `todo` 里

**`weak_points` 是这个接口最有价值的部分**：它回答的不是"我错了几道"，
而是"**我到底哪个概念反复错**"。建议前端重点展示这个。

## 2.10 `GET /api/config` —— 查询当前配置

返回 `{"use_llm": true, "provider": "deepseek"}`，前端可以据此决定要不要显示"AI 批改"相关的 UI。

## 2.11 静态资源 `GET /assets/...`

题目配图。`/api/questions` 返回的 `image` 字段就是这个路径，直接拼域名用。

---

# 第三部分：需要达成的共识（协议 / 约定）

## 3.1 谁负责什么（职责边界）

| | 我们（出题组） | 你们（助学端 / 助教端） |
|---|---|---|
| 学生账号、登录 | ❌ 不做 | ✅ 你们做，把 `student_id` 传给我们 |
| 题目、知识点 | ✅ 我们提供 | 调 `/api/questions` |
| 判分、批改、错因 | ✅ 我们做 | 调 `/api/answer` |
| 作答记录、掌握度 | ✅ 我们存（按 student_id 分开） | 不用重复存 |
| 推题 | ✅ 我们做 | 调 `/api/recommend` |
| 页面 UI | 我们有一套 demo，**可以整个替换** | ✅ 你们做正式 UI |
| 教师-学生的关联关系（班级、任课） | ❌ 不做 | ✅ 你们做 |

## 3.2 数据字典（对齐用）

**章节名（7 个，必须完全一致，多一个字都匹配不上）**：
```
第一章_绪论
第二章_机械加工工艺规程设计
第三章_机床夹具设计
第四章_机械加工精度及其控制
第五章_机械加工表面质量及其控制
第六章_机器装配工艺过程设计
第七章_机械制造工艺理论和技术的发展
```

**题型（5 种）**：`单选` `多选` `名词解释` `简述` `计算`

**认知层级 Bloom（4 种）**：`记忆` `理解` `应用` `分析`

**题目来源（3 种）**：`真题` `教材习题` `AI生成`

**使用范围（2 种）**：`学生练习` `教师出题`
> `教师出题` 的题**绝对不能发给学生**（没有标准答案）

**知识点 ID 格式**：`KN_CH{章号}_{序号}`，如 `KN_CH2_018`

## 3.3 关于知识库（给知识图谱组）

`knowledge_base/` 里是 7 章知识库，**150+ 个知识点**，每个知识点包含：

| 字段 | 说明 |
|---|---|
| `knowledge_id` | 知识点 ID |
| `chapter` | 所属章节 |
| `section_title` | 所属小节 |
| `knowledge_title` | 知识点标题 |
| `content` | 知识点正文（教材原文） |
| `key_concepts` | 关键概念 |
| `formulas` | 公式 |
| `difficulty` | 难度 |
| `prerequisites` | **前置知识点**（学这个之前要先会哪些） |
| `dependencies` | **依赖关系** |
| `learning_order` | 学习顺序 |

**`prerequisites` 和 `dependencies` 是现成的知识图谱边**，知识图谱组可以直接拿去用，
不用重复建。数据在 `db/question_bank.db` 的 `knowledge_points` 表里，一条 SQL 就能导出。

## 3.4 数据库表结构（如果你们要直接读库）

**`questions`（题目表）** —— 关键字段：
```
question_id        题目ID（主键）
course_chapter     章节
source_node_id     知识点ID
question_type      题型：单选/多选/名词解释/简述/计算
stem               题干
options_json       选项（JSON数组，含 is_correct，⚠️不能直接发给学生）
answer             标准答案
explanation        解析
rubric_json        采分点（JSON数组）⚠️ 这是老师的评分标准
total_score        满分
bloom_level        认知层级
image_path         题干配图路径
answer_image_path  ⚠️ 答案图路径（原卷截图题才有，提交后才能给学生）
source             来源：真题/教材习题/AI生成
usage_scope        学生练习 / 教师出题 ⚠️ 后者不能给学生
exam_source        出处（哪年哪张卷子）
review_status      审核状态：已通过/待审核/已驳回 ⚠️ 只有"已通过"能给学生
irt_difficulty_b   IRT 难度值（目前是冷启动估计，不是真实难度）
```

**筛选"能给学生做的题"的标准 SQL**：
```sql
SELECT * FROM questions
WHERE review_status = '已通过'
  AND COALESCE(usage_scope, '学生练习') = '学生练习';
```
**这两个条件缺一不可。** 少了任何一个，都会把没审核过的题或者没答案的题发给学生。

**`student_answers`（作答记录）**：
```
student_id, question_id, mode, student_answer, is_correct, score, time_seconds, created_at
```

**`student_knowledge_mastery`（掌握度）**：
```
student_id, knowledge_id, mastery（0~1）, attempts, corrects, updated_at
```

## 3.5 需要你们确认 / 拍板的事

| # | 问题 | 为什么重要 |
|---|---|---|
| 1 | **`student_id` 用什么？**（学号？UUID？） | 我们只当字符串存，但要全系统统一 |
| 2 | **部署在哪？**我们现在是本地 Python 服务（`serve_quiz.py`），要不要迁到你们的服务器？ | 涉及域名、端口、反向代理 |
| 3 | **数据库要不要合并？**我们现在是独立 SQLite 文件 | 如果你们要统一入库（MySQL/PG），需要有人做迁移 |
| 4 | **UI 是你们做还是用我们的？**我们的 `demo/quiz_page.html` 可以整个替换 | 如果你们做，只需要调我们的 API |
| 5 | **什么时候能接入真实学生？** | ⚠️ **我们最需要的东西**。没有真实作答数据，题目难度就一直是估的，推题准确性上不去 |
| 6 | **教师端和学生端怎么关联？**（班级、任课关系） | 这块我们不做，需要你们定 |
| 7 | **要不要我们提供 Docker 镜像？** | 如果部署环境复杂，我们可以打包 |

## 3.6 已知的坑（提前说，免得对接时踩）

1. **`/api/questions` 不返回答案** —— 这是故意的（防泄题）。答案只能从 `/api/answer` 拿。

2. **`scan: true` 的题，文字题干是残缺的** —— 必须让学生看图。这类题的 `stem` 只是给搜索/分类用的。

3. **`usage_scope='教师出题'` 的题不能给学生** —— 它们没有标准答案。

4. **`review_status != '已通过'` 的题不能给学生** —— 没过审核。

5. **不传 `student_id` 会导致所有人数据混在一起** —— 这是最容易踩的坑。

6. **离线模式下没有智能批改** —— `--provider` 不填时，主观题不会自动判分，
   只会显示标准答案和采分点让学生自评。选择题的判分是不受影响的（本地算）。

7. **题目难度是估计值** —— 见 §2.5。

8. **`options` 只有选择题有** —— 其它题型是 `null`，不要直接 `.map()`。

9. **单选/多选提交的是选项文本，不是字母** —— 见 §2.3。

10. **API 目前没有鉴权** —— 谁都能调。如果要上生产环境，需要加 token（我们可以配合）。

---

# 第四部分：给对面 AI 的快速上手清单

如果你（AI）要帮忙对接，按这个顺序做：

1. **启服务**：`python demo/serve_quiz.py --port 8000`
2. **拉题**：`GET /api/questions` → 拿到题目数组
3. **渲染**：
   - `type` 是 `单选`/`多选` → 用 `options` 渲染选项（多选是复选框）
   - 其它 → 渲染文本框
   - `image` 不为 null → 显示图片
   - `scan` 是 true → 加一句"以图为准"的提示
4. **提交**：`POST /api/answer`，带上 `question_id`、`answer`、`student_id`
   - 单选传选项文本，多选传选项文本用 `；` 拼接
5. **展示反馈**：
   - `is_correct` / `score` → 判分结果
   - `reference_answer` → 标准答案
   - `rubric` → 采分点（让学生对照自评）
   - `answer_image` 不为 null → **显示答案图**（原卷截图题）
   - `process_feedback` → 错因分析
   - `reinforce` → 趁热打铁的同类题
6. **推题**：`GET /api/recommend?student_id=xxx&goal=攻破薄弱`

**最容易出错的三个地方**：
1. 忘了传 `student_id`（数据会混）
2. 单选题提交了 `"A"` 而不是选项文本
3. `scan:true` 的题没提示"以图为准"，学生对着乱码懵了
