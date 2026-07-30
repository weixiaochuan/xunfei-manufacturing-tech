export type LearningAssistantFallbackQuestionType = "choice" | "judgment" | "short_answer";
export type LearningAssistantFallbackDifficulty = "easy" | "medium" | "hard";
export type LearningAssistantFallbackResourceType =
  | "courseware"
  | "case"
  | "checklist"
  | "exercise"
  | "reference";

export interface LearningAssistantFallbackQuestion {
  questionId: string;
  course: string;
  knowledgePoint: string;
  type: LearningAssistantFallbackQuestionType;
  question: string;
  options: string[];
  standardAnswer: string;
  keywords: string[];
  score: number;
  difficulty: LearningAssistantFallbackDifficulty;
  explanation: string;
  sourceTitle: string;
  sourceFile: string;
  tags: string[];
}

export interface LearningAssistantFallbackResource {
  resourceId: string;
  title: string;
  type: LearningAssistantFallbackResourceType;
  course: string;
  knowledgePoint: string;
  difficulty: LearningAssistantFallbackDifficulty;
  summary: string;
  tags: string[];
  duration: string;
  reason: string;
}

interface FallbackSearchInput {
  courseName?: string;
  stageName?: string;
  stageGoal?: string;
  knowledgePoints?: string[];
  currentLevel?: string;
  limit?: number;
}

const FALLBACK_QUESTIONS: LearningAssistantFallbackQuestion[] = [
  {
    questionId: "mfg-q001",
    course: "机械制造工艺学",
    knowledgePoint: "工艺规程设计",
    type: "choice",
    question: "制定机械加工工艺规程时，最先应明确哪一类信息？",
    options: [
      "零件图样、生产纲领和技术要求",
      "机床涂装颜色",
      "操作者个人偏好",
      "仓库摆放位置",
    ],
    standardAnswer: "零件图样、生产纲领和技术要求",
    keywords: ["零件图样", "生产纲领", "技术要求", "工艺规程"],
    score: 10,
    difficulty: "easy",
    explanation: "工艺规程设计需要先明确零件结构、质量要求和生产类型，再确定工艺路线与工序内容。",
    sourceTitle: "制造工艺学 fallback 题库",
    sourceFile: "browser-fallback-questions",
    tags: ["工艺路线", "工序", "入门"],
  },
  {
    questionId: "mfg-q002",
    course: "机械制造工艺学",
    knowledgePoint: "定位基准选择",
    type: "judgment",
    question: "粗基准通常用于首道工序或毛坯表面定位，选择时应兼顾加工余量分配和重要表面位置。",
    options: ["正确", "错误"],
    standardAnswer: "正确",
    keywords: ["粗基准", "首道工序", "加工余量", "重要表面"],
    score: 10,
    difficulty: "medium",
    explanation: "粗基准选择会影响后续加工余量和位置精度，应优先保证重要加工表面的余量均匀。",
    sourceTitle: "制造工艺学 fallback 题库",
    sourceFile: "browser-fallback-questions",
    tags: ["定位", "基准", "夹具"],
  },
  {
    questionId: "mfg-q003",
    course: "机械制造工艺学",
    knowledgePoint: "加工余量",
    type: "short_answer",
    question: "简述加工余量设置过大或过小时可能带来的影响。",
    options: [],
    standardAnswer: "加工余量过大会增加切削时间、材料消耗和变形风险；加工余量过小可能无法去除毛坯缺陷或前道工序误差，影响尺寸精度和表面质量。",
    keywords: ["切削时间", "材料消耗", "变形", "毛坯缺陷", "尺寸精度", "表面质量"],
    score: 20,
    difficulty: "medium",
    explanation: "本题关注加工余量对效率、成本、误差消除和表面质量的综合影响。",
    sourceTitle: "制造工艺学 fallback 题库",
    sourceFile: "browser-fallback-questions",
    tags: ["余量", "精度", "质量"],
  },
  {
    questionId: "mfg-q004",
    course: "机械制造工艺学",
    knowledgePoint: "工序集中与分散",
    type: "choice",
    question: "关于工序集中和工序分散，下列说法更合理的是哪一项？",
    options: [
      "应结合生产类型、设备能力、定位误差和管理成本综合选择",
      "所有零件都必须采用工序集中",
      "所有零件都必须采用工序分散",
      "选择方式只取决于零件名称",
    ],
    standardAnswer: "应结合生产类型、设备能力、定位误差和管理成本综合选择",
    keywords: ["生产类型", "设备能力", "定位误差", "管理成本"],
    score: 10,
    difficulty: "medium",
    explanation: "工序组织方式需要在加工效率、精度保证、设备条件和生产管理之间权衡。",
    sourceTitle: "制造工艺学 fallback 题库",
    sourceFile: "browser-fallback-questions",
    tags: ["工序", "生产类型", "路线"],
  },
  {
    questionId: "mfg-q005",
    course: "机械制造工艺学",
    knowledgePoint: "机械加工精度",
    type: "short_answer",
    question: "列举影响机械加工精度的主要因素，并说明至少一种控制思路。",
    options: [],
    standardAnswer: "主要因素包括机床误差、刀具误差、夹具误差、工件装夹误差、热变形和切削力变形。可通过合理定位夹紧、提高设备精度、补偿热误差和优化切削参数进行控制。",
    keywords: ["机床误差", "刀具误差", "夹具误差", "装夹误差", "热变形", "切削力"],
    score: 20,
    difficulty: "hard",
    explanation: "加工精度分析应覆盖工艺系统误差来源，并给出可执行的控制措施。",
    sourceTitle: "制造工艺学 fallback 题库",
    sourceFile: "browser-fallback-questions",
    tags: ["精度", "误差", "控制"],
  },
  {
    questionId: "mfg-q006",
    course: "机械制造工艺学",
    knowledgePoint: "表面质量",
    type: "judgment",
    question: "表面粗糙度、加工硬化和残余应力都可能影响零件的使用性能。",
    options: ["正确", "错误"],
    standardAnswer: "正确",
    keywords: ["表面粗糙度", "加工硬化", "残余应力", "使用性能"],
    score: 10,
    difficulty: "medium",
    explanation: "表面质量不仅包含几何形貌，也包括表层物理力学性能，会影响耐磨、疲劳和配合性能。",
    sourceTitle: "制造工艺学 fallback 题库",
    sourceFile: "browser-fallback-questions",
    tags: ["表面质量", "粗糙度", "残余应力"],
  },
];

const FALLBACK_RESOURCES: LearningAssistantFallbackResource[] = [
  {
    resourceId: "mfg-r001",
    title: "工艺规程设计入门清单",
    type: "checklist",
    course: "机械制造工艺学",
    knowledgePoint: "工艺规程设计",
    difficulty: "easy",
    summary: "按零件图样、毛坯、生产纲领、工艺路线和工序内容逐项检查工艺规程。",
    tags: ["工艺规程", "工艺路线", "入门"],
    duration: "20分钟",
    reason: "适合在生成学习计划前快速建立规程设计框架。",
  },
  {
    resourceId: "mfg-r002",
    title: "定位基准选择案例复盘",
    type: "case",
    course: "机械制造工艺学",
    knowledgePoint: "定位基准选择",
    difficulty: "medium",
    summary: "围绕粗基准、精基准和基准统一原则，对典型轴套类零件进行判断练习。",
    tags: ["定位基准", "粗基准", "精基准"],
    duration: "30分钟",
    reason: "适合用于测试后补齐基准选择薄弱点。",
  },
  {
    resourceId: "mfg-r003",
    title: "加工余量与尺寸链练习",
    type: "exercise",
    course: "机械制造工艺学",
    knowledgePoint: "加工余量",
    difficulty: "medium",
    summary: "通过余量分配和尺寸链推算练习，理解余量过大或过小对加工质量的影响。",
    tags: ["加工余量", "尺寸链", "练习"],
    duration: "35分钟",
    reason: "适合在错题复盘后作为追加练习。",
  },
  {
    resourceId: "mfg-r004",
    title: "工序集中与分散对比表",
    type: "reference",
    course: "机械制造工艺学",
    knowledgePoint: "工序集中与分散",
    difficulty: "medium",
    summary: "从生产类型、设备条件、定位误差和管理成本四个维度比较两类工序组织方式。",
    tags: ["工序组织", "生产类型", "对比"],
    duration: "15分钟",
    reason: "适合在计划调整时快速判断工序组织策略。",
  },
  {
    resourceId: "mfg-r005",
    title: "加工精度误差来源速查",
    type: "reference",
    course: "机械制造工艺学",
    knowledgePoint: "机械加工精度",
    difficulty: "hard",
    summary: "归纳机床、刀具、夹具、装夹、热变形和切削力变形等误差来源及控制方法。",
    tags: ["加工精度", "误差", "热变形"],
    duration: "25分钟",
    reason: "适合用于高阶复习和主观题答案补充。",
  },
  {
    resourceId: "mfg-r006",
    title: "表面质量影响因素速览",
    type: "courseware",
    course: "机械制造工艺学",
    knowledgePoint: "表面质量",
    difficulty: "medium",
    summary: "解释表面粗糙度、加工硬化、残余应力与零件耐磨性和疲劳性能之间的关系。",
    tags: ["表面质量", "粗糙度", "残余应力"],
    duration: "25分钟",
    reason: "适合用于完成质量控制相关学习任务。",
  },
];

export function findLearningAssistantFallbackQuestions(
  input: FallbackSearchInput = {},
): LearningAssistantFallbackQuestion[] {
  const limit = clampLimit(input.limit, 1, 8);
  const terms = collectSearchTerms(input);
  return FALLBACK_QUESTIONS.map((question) => ({
    item: question,
    score: scoreFallbackItem(question, terms, input),
  }))
    .filter((item) => item.score > 0 || terms.length === 0)
    .sort(sortScoredItems)
    .slice(0, limit)
    .map((item) => cloneQuestion(item.item));
}

export function findLearningAssistantFallbackResources(
  input: FallbackSearchInput = {},
): LearningAssistantFallbackResource[] {
  const limit = clampLimit(input.limit, 1, 6);
  const terms = collectSearchTerms(input);
  return FALLBACK_RESOURCES.map((resource) => ({
    item: resource,
    score: scoreFallbackItem(resource, terms, input),
  }))
    .filter((item) => item.score > 0 || terms.length === 0)
    .sort(sortScoredItems)
    .slice(0, limit)
    .map((item) => cloneResource(item.item));
}

export function getLearningAssistantFallbackQuestionBank(): LearningAssistantFallbackQuestion[] {
  return FALLBACK_QUESTIONS.map(cloneQuestion);
}

export function getLearningAssistantFallbackResourceBank(): LearningAssistantFallbackResource[] {
  return FALLBACK_RESOURCES.map(cloneResource);
}

function cloneQuestion(question: LearningAssistantFallbackQuestion): LearningAssistantFallbackQuestion {
  return {
    ...question,
    options: [...question.options],
    keywords: [...question.keywords],
    tags: [...question.tags],
  };
}

function cloneResource(resource: LearningAssistantFallbackResource): LearningAssistantFallbackResource {
  return {
    ...resource,
    tags: [...resource.tags],
  };
}

function scoreFallbackItem(
  item: LearningAssistantFallbackQuestion | LearningAssistantFallbackResource,
  terms: string[],
  input: FallbackSearchInput,
): number {
  const haystack = normalizeSearchText(
    [
      item.course,
      item.knowledgePoint,
      "title" in item ? item.title : "",
      "question" in item ? item.question : item.summary,
      ...item.tags,
    ].join(" "),
  );
  let score = normalizeSearchText(input.courseName).includes(normalizeSearchText(item.course)) ? 2 : 0;
  for (const term of terms) {
    if (!term) continue;
    if (normalizeSearchText(item.knowledgePoint).includes(term)) score += 6;
    else if (haystack.includes(term)) score += 2;
  }
  if (/零基础|基础较弱|入门|beginner|weak/i.test(input.currentLevel ?? "") && item.difficulty === "easy") {
    score += 1;
  }
  return score;
}

function sortScoredItems<T extends { knowledgePoint: string; resourceId?: string; questionId?: string }>(
  left: { item: T; score: number },
  right: { item: T; score: number },
): number {
  return (
    right.score - left.score ||
    left.item.knowledgePoint.localeCompare(right.item.knowledgePoint, "zh-Hans-CN") ||
    (left.item.questionId ?? left.item.resourceId ?? "").localeCompare(
      right.item.questionId ?? right.item.resourceId ?? "",
    )
  );
}

function collectSearchTerms(input: FallbackSearchInput): string[] {
  return uniqueStrings(
    [
      input.courseName,
      input.stageName,
      input.stageGoal,
      ...(input.knowledgePoints ?? []),
    ]
      .flatMap((value) => splitSearchTerms(value))
      .filter(Boolean),
  ).slice(0, 16);
}

function splitSearchTerms(value: string | undefined): string[] {
  const normalized = normalizeSearchText(value);
  if (!normalized) return [];
  return normalized
    .split(/[、，,；;：:\s/()（）-]+/)
    .map((item) => item.trim())
    .filter((item) => item.length >= 2);
}

function normalizeSearchText(value: string | undefined): string {
  return (value ?? "")
    .normalize("NFKC")
    .trim()
    .toLocaleLowerCase("zh-Hans-CN")
    .replace(/\s+/g, " ");
}

function clampLimit(value: number | undefined, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) return max;
  return Math.max(min, Math.min(max, value));
}

function uniqueStrings(values: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const item = value.trim();
    const key = item.toLocaleLowerCase("zh-Hans-CN");
    if (!item || seen.has(key)) continue;
    seen.add(key);
    result.push(item);
  }
  return result;
}
