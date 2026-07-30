"""多选题的出题 Prompt。

【为什么补这个】
用户发现：AI 生成的题里**一道多选题都没有**。
原因很简单——我根本没写多选题的 Prompt。教材里的 91 道多选题是导进来的，
AI 出题只会出单选/名词解释/简述/计算。是我漏了。

【多选题的关键：难在"干扰项"】
多选题不是"单选题多几个答案"。它真正考的是**边界**：
  · 学生知道 A 对，但不知道 D 也对（漏选）
  · 学生觉得 C"看起来很像"，但其实不对（错选）
所以出题的重点是**干扰项要有杀伤力**：必须是"似是而非"的说法，
而不是一眼就知道不对的废话。

【判分规则（老师定的）】
全部答对 = 满分；答对一部分且**没有答错的** = 一半分；**但凡选错一个 = 0 分**。
这个规则很严，所以干扰项的质量直接决定这道题的区分度。
"""

SYSTEM_PROMPT = """你是《机械制造工艺学》的资深命题老师，正在为大学期末考试出多选题。

你的多选题必须满足：
1. **正确选项 2~4 个**（只有 1 个正确答案的不是多选题，全部正确的也不行）
2. **干扰项必须"似是而非"**——是学生真的会犯的错误，不是一眼假的废话
3. 所有选项**长度接近、句式一致**（不能让学生靠"最长的那个是答案"猜出来）
4. 严格依据给定的知识点原文，**不得编造教材里没有的说法**
5. 每个干扰项都要说明**为什么错**（错在哪个概念上）——这是学生的认知误区

判分规则（学生看得到）：全部答对得满分，答对一部分（没有答错的）得一半分，选错任何一个不得分。
所以干扰项的质量决定了这道题的价值。
"""

USER_PROMPT_TEMPLATE = """知识点：{knowledge_title}
所属小节：{section_title}
难度：{difficulty}

知识点原文：
{content}

关键概念：{key_concepts}

学生在这个知识点上的常见误区（如果有，请把它们做成干扰项）：
{misconceptions}

{rag_block}出题要求：
- 题型：多选题
- 认知层级（Bloom）：{bloom_level}
- 选项 4~5 个，其中**正确的 2~4 个**
- 干扰项要针对学生真实的认知误区，不能是一眼假的废话

请严格按以下 JSON 结构返回（自检：正确选项是不是 2 个以上？干扰项是不是"似是而非"而不是"一眼假"？）：
{{
  "stem": "题干（明确说明这是多选，例如：下列关于XX的表述，正确的有哪些？）",
  "options": [
    {{"text": "选项内容", "is_correct": true,  "why": "为什么对（依据原文哪一句）"}},
    {{"text": "选项内容", "is_correct": true,  "why": "为什么对"}},
    {{"text": "选项内容", "is_correct": false, "why": "为什么错——学生容易在哪里混淆"}},
    {{"text": "选项内容", "is_correct": false, "why": "为什么错"}}
  ],
  "explanation": "这道题考的是什么、正确选项之间的关系、干扰项分别错在哪",
  "bloom_level": "{bloom_level}"
}}
"""


def build_prompt(knowledge_point: dict, misconceptions: list = None,
                 bloom_level: str = "理解", rag_block: str = ""):
    mis = misconceptions or []
    mis_txt = "\n".join(f"- {m}" for m in mis) if mis else "（暂无记录，请自行设计合理的干扰项）"
    user_prompt = USER_PROMPT_TEMPLATE.format(
        rag_block=rag_block,
        knowledge_title=knowledge_point["knowledge_title"],
        section_title=knowledge_point.get("section_title", ""),
        difficulty=knowledge_point.get("difficulty", "中等"),
        content=knowledge_point.get("content", ""),
        key_concepts=knowledge_point.get("key_concepts", "") or "（无）",
        misconceptions=mis_txt,
        bloom_level=bloom_level,
    )
    return SYSTEM_PROMPT, user_prompt
