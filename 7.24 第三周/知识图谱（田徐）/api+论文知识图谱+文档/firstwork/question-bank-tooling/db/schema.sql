-- ============================================================
-- 题库系统数据库schema
-- 依据：《基于知识库的学-练-考助学支持模块调研》第6.2节 五层字段设计
-- ============================================================

PRAGMA foreign_keys = ON;

-- ------------------------------------------------------------
-- 1. 知识点表（知识图谱节点）
--    直接对应两章知识库Excel的15个字段，是出题的原文来源（RAG锚定）
-- ------------------------------------------------------------
CREATE TABLE IF NOT EXISTS knowledge_points (
    knowledge_id     TEXT PRIMARY KEY,      -- 如 KN_CH1_001
    chapter          TEXT NOT NULL,         -- 从文件名/knowledge_id前缀推断，如 第一章_绪论
    section_title    TEXT,
    knowledge_title  TEXT,
    content          TEXT,                  -- 原文片段，出题Prompt的RAG依据
    key_concepts     TEXT,                  -- 分号分隔
    formulas         TEXT,                  -- 分号分隔，计算题验算依据
    figures          TEXT,
    difficulty       TEXT,                  -- 基础/中等/困难（知识点本身难度，非题目难度）
    knowledge_type   TEXT,                  -- 概念/概述/公式/方法...
    prerequisites    TEXT,                  -- 前置知识点ID，逗号分隔
    dependencies     TEXT,                  -- 后续依赖知识点ID，逗号分隔
    tags             TEXT,
    page             INTEGER,
    learning_order   INTEGER,
    metadata         TEXT                   -- 原始JSON字符串
);

-- ------------------------------------------------------------
-- 2. 典型认知误区库
--    对应报告2.4节(1)："先确定认知误区，再反推干扰项"（学科网逻辑）
--    每个知识点可关联多条误区，出题时从中抽取生成干扰项
-- ------------------------------------------------------------
CREATE TABLE IF NOT EXISTS misconceptions (
    misconception_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    knowledge_id        TEXT NOT NULL REFERENCES knowledge_points(knowledge_id),
    misconception_text  TEXT NOT NULL,       -- 误区描述，如"测量基准必须始终与设计基准重合"
    source               TEXT DEFAULT 'seed', -- seed=人工种子 / llm_mined=模型从答题数据归纳
    created_at           TEXT DEFAULT (datetime('now'))
);

-- ------------------------------------------------------------
-- 3. 题目表（五层字段设计）
-- ------------------------------------------------------------
CREATE TABLE IF NOT EXISTS questions (
    -- ---- 基础信息层 ----
    question_id       TEXT PRIMARY KEY,      -- 全局唯一ID，如 Q_000001
    course_chapter    TEXT,
    source_node_id    TEXT NOT NULL REFERENCES knowledge_points(knowledge_id),  -- 秘塔模式：来源可追溯
    question_type     TEXT NOT NULL,         -- 单选/多选/判断/填空/计算/案例分析
    stem              TEXT NOT NULL,         -- 题干
    options_json      TEXT,                  -- JSON数组，含每个选项文本+对应认知误区标注
    answer            TEXT NOT NULL,
    explanation       TEXT,

    -- ---- 教学目标层 ----
    bloom_level        TEXT,                 -- 记忆/理解/应用/分析/评价/创造
    prerequisite_ids    TEXT,                -- 逗号分隔的knowledge_id
    target_ability      TEXT,                -- 本题考察能力描述

    -- ---- 难度参数层 ----
    subjective_difficulty TEXT,              -- 易/中/难，出题时设定
    irt_difficulty_b       REAL,             -- 上线初期留空，积累30-50条答题记录后标定
    irt_discrimination_a    REAL,

    -- ---- AI生成层 ----
    generation_model     TEXT,               -- 生成该题所用模型名称+版本
    prompt_template_id    TEXT,
    review_status          TEXT DEFAULT '待审核',  -- 待审核/已通过/已驳回
    calc_verify_status      TEXT DEFAULT '无需验算', -- 无需验算/已验算通过/验算失败
    calc_verify_detail       TEXT,           -- 验算过程记录，便于人工复核

    -- ---- 生产、审核和媒体扩展 ----
    explanation_old        TEXT,
    image_path              TEXT,
    image_reviewed          INTEGER DEFAULT 0,
    rubric_json             TEXT,
    total_score             REAL,
    source                  TEXT DEFAULT 'AI生成',
    usage_scope             TEXT DEFAULT '学生练习',
    no_answer_reason        TEXT,
    exam_source             TEXT,
    answer_source           TEXT,
    answer_image_path       TEXT,

    created_at TEXT DEFAULT (datetime('now')),

    -- ---- 学生行为层（动态，初始化为空/0）----
    answer_count      INTEGER DEFAULT 0,
    correct_rate       REAL,
    avg_time_seconds     INTEGER,
    common_error_tags     TEXT,              -- 预定义典型认知误区标签，来自本题实际答题归纳
    recommend_count       INTEGER DEFAULT 0
  );

  CREATE INDEX IF NOT EXISTS idx_questions_student_scope
      ON questions(review_status, usage_scope, course_chapter);

-- 一题可关联多个知识点（多对多），source_node_id 只记录"主"来源
CREATE TABLE IF NOT EXISTS question_knowledge_map (
    question_id    TEXT NOT NULL REFERENCES questions(question_id),
    knowledge_id   TEXT NOT NULL REFERENCES knowledge_points(knowledge_id),
    PRIMARY KEY (question_id, knowledge_id)
);

-- ------------------------------------------------------------
-- 4. 学生答题记录（Phase 1 即建表，为Phase 2/3的知识追踪、IRT标定做数据积累）
-- ------------------------------------------------------------
CREATE TABLE IF NOT EXISTS student_answers (
    answer_id        INTEGER PRIMARY KEY AUTOINCREMENT,
    student_id        TEXT NOT NULL,
    question_id        TEXT NOT NULL REFERENCES questions(question_id),
    mode                TEXT DEFAULT '练习',   -- 练习/测试，决定反馈是否即时展示解析
    student_answer       TEXT,
    is_correct             INTEGER,           -- 0/1
    time_seconds            INTEGER,
    is_redo                   INTEGER DEFAULT 0,
    error_type_tag             TEXT,          -- 命中的常见错误类型
    feedback_json               TEXT,         -- {错因, 建议行动, 推荐复习知识点ID}（练习模式）
    answered_at TEXT DEFAULT (datetime('now'))
);

-- ------------------------------------------------------------
-- 5. 学生知识点掌握度画像（Phase 1 初始化空，Phase 3接入BKT/DKT实时更新）
-- ------------------------------------------------------------
CREATE TABLE IF NOT EXISTS student_knowledge_mastery (
    student_id     TEXT NOT NULL,
    knowledge_id    TEXT NOT NULL REFERENCES knowledge_points(knowledge_id),
    mastery_prob     REAL DEFAULT 0.0,        -- 0~1，知识追踪模型输出
    updated_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY (student_id, knowledge_id)
);

CREATE INDEX IF NOT EXISTS idx_questions_source_node ON questions(source_node_id);
CREATE INDEX IF NOT EXISTS idx_questions_review_status ON questions(review_status);
CREATE INDEX IF NOT EXISTS idx_answers_student ON student_answers(student_id);
