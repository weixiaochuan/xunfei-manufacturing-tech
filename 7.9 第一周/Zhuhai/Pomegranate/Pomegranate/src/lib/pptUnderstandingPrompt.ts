export interface PptUnderstandingPromptInput {
  topic: string;
  sourceMaterial: string;
  audience: string;
  pageCount: string;
  style: string;
  extraRequirements: string;
}

export interface PptUnderstandingPromptParts {
  rawMaterial: string;
  promptText: string;
  metadataText: string;
  fullPrompt: string;
}

const UNDERSTANDING_PROMPT_INTRO = `你是一名 PPT 策划专家和比赛汇报教练。用户会提供 PPT 主题、素材、汇报对象、页数、风格和额外要求。你的任务不是复述资料，而是判断这份 PPT 应该如何组织，生成一份“用户可确认的 PPT 制作理解结果”。

请输出中文，结构清晰，尽量精简。不要大段复制原始语料。重点回答：

1. 这份 PPT 的核心目标是什么？
2. 应该讲给谁听？听众最关心什么？
3. 这份材料里最值得突出的 3-5 个重点是什么？
4. 哪些内容应该弱化或合并？
5. 推荐的叙事主线是什么？
6. 建议页面结构，每页一句话说明。
7. 推荐视觉风格和版式倾向。
8. 如果素材确实缺少必须由用户补充或决定的信息，再指出还缺什么；页面结构、内容比例和版式设计由你直接完成，不要反问用户。

用户输入如下：
`;

const UNDERSTANDING_PROMPT_OUTPUT_CONTRACT = `

输出格式必须是：

通用格式要求：如果字段内包含多个编号或项目符号，每一项必须单独占一行，使用 Markdown 编号或项目符号；不得把“1. …… 2. …… 3. ……”挤在同一行。叙事主线包含多个阶段时也要分行。JSON 之外的普通标题和自然段按下面格式输出。

【AI理解摘要】
用 2-4 句话概括你认为这份 PPT 应该做成什么样。

【重点取舍】
* 应突出：
  1.
  2.
  3.
* 应弱化/合并：
  1.
  2.

【叙事主线】
说明推荐的讲述逻辑；如果包含多个阶段，每个阶段单独一行。

【建议页面结构】
1. xxx：一句话说明
2. xxx：一句话说明
...

【视觉与表达建议】
用 2-4 条短句逐项说明风格、版式和视觉表达。信息层级、页面比例、图文图表安排和视觉平衡必须由系统直接给出方案。

【仍需确认的问题】
这里只能列出必须由用户补充、确认或承担价值判断的信息，例如素材缺失但生成依赖的事实、互相冲突的数据取舍、汇报立场、敏感个人信息、必须强调或禁止涉及的内容、无法从素材判断的具体受众要求、争议性数据授权。
不得询问页面怎么排版、哪一页放什么、各部分比例、是否合并页面、页面是否拥挤、时间线还是卡片、图文图表安排、信息层级、视觉平衡或结论页布局；这些必须在重点取舍、叙事主线、建议页面结构或视觉与表达建议中直接解决。
不要为了填满字段制造问题，不强制问题数量。
如果不存在真正需要用户补充的信息，必须写“暂无需要用户补充的信息，系统将根据现有材料自动完成内容组织与版式规划。”`;

function buildUnderstandingMetadata(values: PptUnderstandingPromptInput): string {
  return `【主题】
${values.topic}

【汇报对象】
${values.audience}

【页数】
${values.pageCount}

【风格】
${values.style}

【额外要求】
${values.extraRequirements}

【原始语料】
`;
}

/** 真实 AI 理解请求与容量估算共享同一份 Prompt Builder，避免两套模板漂移。 */
export function buildAiUnderstandingPromptParts(
  values: PptUnderstandingPromptInput,
): PptUnderstandingPromptParts {
  const metadataText = buildUnderstandingMetadata(values);
  const promptText = UNDERSTANDING_PROMPT_INTRO + UNDERSTANDING_PROMPT_OUTPUT_CONTRACT;
  return {
    rawMaterial: values.sourceMaterial,
    promptText,
    metadataText,
    fullPrompt:
      UNDERSTANDING_PROMPT_INTRO +
      metadataText +
      values.sourceMaterial +
      UNDERSTANDING_PROMPT_OUTPUT_CONTRACT,
  };
}

export function buildAiUnderstandingPrompt(values: PptUnderstandingPromptInput): string {
  return buildAiUnderstandingPromptParts(values).fullPrompt;
}
