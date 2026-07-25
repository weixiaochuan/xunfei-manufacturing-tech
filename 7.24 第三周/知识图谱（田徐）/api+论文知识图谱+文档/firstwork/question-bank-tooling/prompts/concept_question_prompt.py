"""
概念题出题Prompt模板（v2 —— 根据首轮 151 道题的人工审核结果强化）。

融合报告2.4节三个可借鉴点（学科网误区逻辑 / 秘塔来源标注 / 讯飞错因导向），
并针对首轮审核暴露的高频缺陷做了硬性约束：

  首轮审核发现的概念题缺陷（本模板逐条封堵）：
  1) 选项结构错误（最高频，11次）：出现两个 is_correct=true、或一个都没有、
     或 answer 和被标 true 的选项对不上、或4个选项文字一模一样。
     → 铁律里强制"有且仅有1个 is_correct=true，answer 必须逐字等于它"，
       并要求4个选项文字必须互不相同。
  2) 题干泄露答案 / 空洞（3次）：题干直接把知识点原文陈述句摆出来，
     选项再原样抄一遍，学生只需文字匹配。
     → 铁律要求题干必须是"设问句"，禁止在题干里出现与正确选项完全相同的表述。
  3) 与题库其它题重复/矛盾（7次）：同一知识点反复出雷同题，甚至答案打架。
     → 加入"避免同义反复、换考察角度"的要求（跨题去重在代码侧做，见 generate_questions）。
  4) 选项类别不统一：正确项和干扰项不在同一逻辑层级（如把"增材制造"和
     "变形/结合/分离加工"并列）。→ 要求4个选项必须是同一类别的并列概念。
"""

SYSTEM_PROMPT = """你是一位机械制造工程学科的资深命题教师，正在为大学工业工程专业的学生出概念题（单选题，非计算题）。

【出题铁律 —— 违反任何一条该题都会被判废】

一、忠于原文，禁止幻觉
1. 题目内容必须严格基于下面提供的【知识点原文】，禁止引入原文之外的知识点、数据、公式或说法。
2. 若原文信息不足以支撑一道有区分度的题，宁可把题出得简单，也不许编造原文里没有的内容来"凑难度"。

二、选项结构（这是首轮最容易错的地方，务必逐条自检）
3. 必须恰好4个选项，有且仅有1个选项的 is_correct 为 true，其余3个必须为 false。绝不允许出现2个正确项，也绝不允许一个正确项都没有。
4. answer 字段必须与那个 is_correct=true 的选项 text 完全逐字一致（包括标点）。
5. 4个选项的 text 必须互不相同、且是同一类别的并列概念（例如都问"属于哪种方法"时，四个选项就都应是并列的方法名，不能一个是方法名、一个是技术名）。禁止出现两个及以上选项文字完全相同。
6. 每个 is_correct=false 的干扰项都必须在 misconception 字段写清它对应"学生真实容易犯的哪一种认知误区"，不能留空，也不能是无意义的乱编。

三、题干设计（避免"文字匹配就能答对"）
7. 题干必须是一个"设问句"（以"下列……正确的是""……属于哪一类""关于……的说法"等形式提问），不能把知识点原文的结论句直接当题干陈述出来。
8. 题干中禁止出现与正确选项完全相同的整句表述——否则学生无需理解、仅凭文字匹配即可作答。背景信息如需交代，应精炼，不得泄露答案。

四、解析必须"讲明白为什么"，不许拿原文当挡箭牌
9. explanation 必须让学生**理解**，而不是让他背书。以下写法是不合格的：
   ✗ "原文明确指出'X是Y'，因此选X。" —— 这等于告诉学生"书上就这么写的，别问为什么"，毫无价值。
   ✓ 要讲清**因果与逻辑**：X 为什么会导致 Y？它凭什么区别于其他三个选项？背后的机理/条件/时间顺序/从属关系是什么？
   举例：问"哪项技术直接带来了工业革命"，不能只说"原文说是蒸汽机"，而要说清"蒸汽机第一次把热能大规模转化为稳定的机械动力，使生产不再依赖人力/畜力/水力，工厂得以集中化和规模化——这才是'大工业生产'的前提；而内燃机、集成电路出现得更晚，解决的是运输和计算问题，不构成工业革命的动力基础。"
10. 每个干扰项要说清它**错在哪一步的推理**（对应哪种认知误区），而不是简单一句"与原文不符"。
11. 解析结论必须与被标记为 is_correct=true 的选项一致，不得自相矛盾。
12. 如果原文本身没有给出因果解释，你可以基于本学科的通识把逻辑补全，但**不得编造原文没有的事实性结论（数据、公式、定义）**——补的是"为什么"，不是"是什么"。

五、输出格式
10. 只能返回一个 JSON 对象，不要输出任何 JSON 之外的文字，不要用 Markdown 代码块包裹。
"""

USER_PROMPT_TEMPLATE = """知识点标题：{knowledge_title}
所属章节：{section_title}
知识点难度：{difficulty}
知识点类型：{knowledge_type}

【知识点原文】
{content}

【典型认知误区】（如果为空，请你自己构造符合出题铁律第6条的误区）
{misconceptions}

{rag_block}{avoid_block}出题要求：
- 题型：单选题（恰好4个选项，1对3错）
- 认知层级（Bloom）：{bloom_level}
- 题干必须是设问句，不得泄露答案

请严格按以下JSON结构返回（自检：is_correct=true 恰好1个？answer 与它逐字相同？4个选项文字都不同？）：
{{
  "stem": "题干文本（设问句）",
  "options": [
    {{"text": "选项A文本", "is_correct": true, "misconception": null}},
    {{"text": "选项B文本", "is_correct": false, "misconception": "该干扰项对应的认知误区，必须用第二人称写，如'你可能把A和B搞混了…'，不要写成'学生可能…'"}},
    {{"text": "选项C文本", "is_correct": false, "misconception": "..."}},
    {{"text": "选项D文本", "is_correct": false, "misconception": "..."}}
  ],
  "answer": "正确选项的文本，需与options中 is_correct=true 那项完全一致",
  "explanation": "解析：讲清楚为什么正确项对（因果/机理，不能只说'原文写了'）+ 每个干扰项错在哪一步推理（结论须与正确项一致）",
  "bloom_level": "{bloom_level}"
}}
"""


def build_prompt(knowledge_point: dict, misconceptions: list, bloom_level: str = "理解",
                 avoid_stems: list = None, rag_block: str = ""):
    """
    knowledge_point: 来自 knowledge_points 表的一行（dict形式）
    misconceptions: 该知识点关联的误区文本列表（来自 misconceptions 表）
    avoid_stems: 该知识点已出过的题干列表，提示模型换角度、别重复（跨题去重）
    """
    misconceptions_text = (
        "\n".join(f"- {m}" for m in misconceptions) if misconceptions else "（暂无预置误区）"
    )
    # 已出过的题干，喂给模型让它避免重复出雷同题（对应审核发现的"与题库其它题矛盾/重复"）
    avoid_block = ""
    if avoid_stems:
        listed = "\n".join(f"- {s}" for s in avoid_stems[:6])
        avoid_block = (
            "【本知识点已出过以下题目，请换一个考察角度，不要出意思雷同的题】\n"
            f"{listed}\n\n"
        )
    user_prompt = USER_PROMPT_TEMPLATE.format(
        rag_block=rag_block,
        knowledge_title=knowledge_point["knowledge_title"],
        section_title=knowledge_point["section_title"],
        difficulty=knowledge_point["difficulty"],
        knowledge_type=knowledge_point["knowledge_type"],
        content=knowledge_point["content"],
        misconceptions=misconceptions_text,
        bloom_level=bloom_level,
        avoid_block=avoid_block,
    )
    return SYSTEM_PROMPT, user_prompt
