import type { JsonObject } from "./accountLearningTypes.ts";

export const LEARNING_ASSISTANT_QUIZ_SOURCE = "local-fallback-quiz";
export const LEARNING_ASSISTANT_QUIZ_RECORD_LIMIT = 20;

export type LearningAssistantQuizQuestionType = "choice" | "judgment" | "short_answer";
export type LearningAssistantQuizDifficulty = "easy" | "medium" | "hard";

export interface LearningAssistantQuizStageSnapshot {
  name?: string;
  goal?: string;
  knowledgePoints?: string[];
  learningEntries?: LearningAssistantQuizEntrySnapshot[];
  learningTasks?: string[];
  practiceTasks?: string[];
  checkTasks?: string[];
}

export interface LearningAssistantQuizEntrySnapshot {
  title?: string;
  section?: string;
  masteryLevel?: string;
  studyAction?: string;
  practiceAction?: string;
  checkMethod?: string;
  expectedOutput?: string;
  reason?: string;
  sourceFile?: string;
}

export interface LearningAssistantQuizQuestion {
  questionKey: string;
  stageIndex: number;
  stageName: string;
  knowledgePoint: string;
  type: LearningAssistantQuizQuestionType;
  question: string;
  options: string[];
  standardAnswer: string;
  keywords: string[];
  score: number;
  difficulty: LearningAssistantQuizDifficulty;
  explanation: string;
  sourceTitle?: string;
  sourceFile?: string;
}

export interface LearningAssistantQuizAnswer {
  questionKey: string;
  userAnswer: string;
}

export interface LearningAssistantQuizDetailResult {
  questionKey: string;
  userAnswer: string;
  standardAnswer: string;
  score: number;
  maxScore: number;
  correct: boolean;
  missingKeywords: string[];
  comment: string;
}

export interface LearningAssistantQuizScoreResult {
  totalScore: number;
  maxScore: number;
  percentage: number;
  level: "优秀" | "基本掌握" | "需要重学";
  weakKnowledgePoints: string[];
  missingKeywords: string[];
  feedback: string;
  suggestions: string[];
  canAdvance: boolean;
  detailResults: LearningAssistantQuizDetailResult[];
}

export interface LearningAssistantQuizRecordItem {
  questionKey: string;
  question: string;
  questionType: LearningAssistantQuizQuestionType;
  options: string[];
  userAnswer: string;
  standardAnswer: string;
  explanation: string;
  score: number;
  maxScore: number;
  correct: boolean;
  knowledgePoint: string;
  missingKeywords: string[];
}

export interface LearningAssistantQuizRecord {
  recordKey: string;
  source: typeof LEARNING_ASSISTANT_QUIZ_SOURCE;
  testedAt: string;
  stageIndex: number;
  stageName: string;
  totalScore: number;
  maxScore: number;
  percentage: number;
  level: LearningAssistantQuizScoreResult["level"];
  weakKnowledgePoints: string[];
  missingKeywords: string[];
  feedback: string;
  suggestions: string[];
  canAdvance: boolean;
  items: LearningAssistantQuizRecordItem[];
}

interface QuizCandidate {
  title: string;
  section?: string;
  action?: string;
  checkMethod?: string;
  reason?: string;
  sourceFile?: string;
}

export function buildLearningAssistantStageQuiz(input: {
  stage: LearningAssistantQuizStageSnapshot;
  stageIndex: number;
  currentLevel?: string;
  limit?: number;
}): LearningAssistantQuizQuestion[] {
  const stageName = clean(input.stage.name) || `阶段 ${input.stageIndex + 1}`;
  const candidates = collectQuizCandidates(input.stage);
  const limit = clampInteger(input.limit ?? 5, 1, 8);
  return candidates.slice(0, limit).map((candidate, index) => {
    const type = questionTypeForIndex(index);
    const difficulty = difficultyForLevel(input.currentLevel, index);
    const score = type === "short_answer" ? 20 : 10;
    const keywords = buildKeywords(candidate);
    return {
      questionKey: makeQuestionKey(input.stageIndex, candidate.title, index),
      stageIndex: input.stageIndex,
      stageName,
      knowledgePoint: candidate.title,
      type,
      question: buildQuestionText(type, candidate, stageName),
      options: buildOptions(type, candidate, input.stage.goal),
      standardAnswer: buildStandardAnswer(type, candidate, input.stage.goal),
      keywords,
      score,
      difficulty,
      explanation: buildExplanation(type, candidate, stageName),
      sourceTitle: candidate.section,
      sourceFile: candidate.sourceFile,
    };
  });
}

export function scoreLearningAssistantQuiz(
  questions: LearningAssistantQuizQuestion[],
  answers: LearningAssistantQuizAnswer[],
): LearningAssistantQuizScoreResult {
  const answerMap = new Map(answers.map((answer) => [answer.questionKey, answer.userAnswer]));
  const detailResults = questions.map((question) =>
    scoreLearningAssistantQuestion(question, answerMap.get(question.questionKey) ?? ""),
  );
  const totalScore = detailResults.reduce((sum, item) => sum + item.score, 0);
  const maxScore = detailResults.reduce((sum, item) => sum + item.maxScore, 0);
  const percentage = getLearningQuizPercentage(totalScore, maxScore);
  const weakKnowledgePoints = uniqueStrings(
    questions
      .filter((question) => {
        const detail = detailResults.find((item) => item.questionKey === question.questionKey);
        return detail ? detail.score < detail.maxScore : false;
      })
      .map((question) => question.knowledgePoint),
  );
  const missingKeywords = uniqueStrings(detailResults.flatMap((detail) => detail.missingKeywords));
  const { level, feedback } = getLearningQuizLevel(percentage);

  return {
    totalScore,
    maxScore,
    percentage,
    level,
    weakKnowledgePoints,
    missingKeywords,
    feedback,
    suggestions: buildLearningQuizSuggestions(percentage, weakKnowledgePoints),
    canAdvance: percentage >= 60,
    detailResults,
  };
}

export function buildLearningAssistantQuizRecord(input: {
  stage: LearningAssistantQuizStageSnapshot;
  stageIndex: number;
  questions: LearningAssistantQuizQuestion[];
  answers: Record<string, string>;
  scoreResult: LearningAssistantQuizScoreResult;
  testedAt?: string;
}): LearningAssistantQuizRecord {
  const testedAt = input.testedAt ?? new Date().toISOString();
  const stageName = clean(input.stage.name) || `阶段 ${input.stageIndex + 1}`;
  return {
    recordKey: makeRecordKey(input.stageIndex, testedAt),
    source: LEARNING_ASSISTANT_QUIZ_SOURCE,
    testedAt,
    stageIndex: input.stageIndex,
    stageName,
    totalScore: input.scoreResult.totalScore,
    maxScore: input.scoreResult.maxScore,
    percentage: input.scoreResult.percentage,
    level: input.scoreResult.level,
    weakKnowledgePoints: input.scoreResult.weakKnowledgePoints,
    missingKeywords: input.scoreResult.missingKeywords,
    feedback: input.scoreResult.feedback,
    suggestions: input.scoreResult.suggestions,
    canAdvance: input.scoreResult.canAdvance,
    items: input.questions.map((question) => {
      const detail = input.scoreResult.detailResults.find(
        (item) => item.questionKey === question.questionKey,
      );
      return {
        questionKey: question.questionKey,
        question: question.question,
        questionType: question.type,
        options: question.options,
        userAnswer: input.answers[question.questionKey] ?? "",
        standardAnswer: question.standardAnswer,
        explanation: question.explanation,
        score: detail?.score ?? 0,
        maxScore: detail?.maxScore ?? question.score,
        correct: detail?.correct ?? false,
        knowledgePoint: question.knowledgePoint,
        missingKeywords: detail?.missingKeywords ?? [],
      };
    }),
  };
}

export function appendLearningAssistantQuizRecordToProgress(
  progress: JsonObject | null | undefined,
  record: LearningAssistantQuizRecord,
  limit = LEARNING_ASSISTANT_QUIZ_RECORD_LIMIT,
): JsonObject {
  const previous = isRecord(progress) ? progress : {};
  const records = extractLearningAssistantQuizRecords(previous);
  const nextRecords = [
    record,
    ...records.filter((item) => item.recordKey !== record.recordKey),
  ].slice(0, limit);
  return {
    ...previous,
    quizRecords: nextRecords,
    quizRecordCount: nextRecords.length,
    latestQuizAt: record.testedAt,
    latestQuizPercentage: record.percentage,
    latestWeakKnowledgePoints: record.weakKnowledgePoints,
  };
}

export function extractLearningAssistantQuizRecords(
  progress: JsonObject | null | undefined,
): LearningAssistantQuizRecord[] {
  if (!isRecord(progress) || !Array.isArray(progress.quizRecords)) return [];
  return progress.quizRecords
    .map(parseQuizRecord)
    .filter((record): record is LearningAssistantQuizRecord => record !== null);
}

export function getLearningQuizPercentage(totalScore: number, maxScore: number): number {
  if (!Number.isFinite(totalScore) || !Number.isFinite(maxScore) || maxScore <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((totalScore / maxScore) * 100)));
}

function scoreLearningAssistantQuestion(
  question: LearningAssistantQuizQuestion,
  userAnswer: string,
): LearningAssistantQuizDetailResult {
  const normalizedUserAnswer = normalizeQuizAnswer(userAnswer);
  const normalizedStandardAnswer = normalizeQuizAnswer(question.standardAnswer);

  if (question.type === "choice" || question.type === "judgment") {
    const correct = normalizedUserAnswer === normalizedStandardAnswer;
    return {
      questionKey: question.questionKey,
      userAnswer,
      standardAnswer: question.standardAnswer,
      score: correct ? question.score : 0,
      maxScore: question.score,
      correct,
      missingKeywords: [],
      comment: correct ? "答案正确" : "答案与标准答案不一致",
    };
  }

  const answerLower = userAnswer.toLocaleLowerCase();
  const matchedCount = question.keywords.filter((keyword) =>
    answerLower.includes(keyword.toLocaleLowerCase()),
  ).length;
  const missingKeywords = question.keywords.filter(
    (keyword) => !answerLower.includes(keyword.toLocaleLowerCase()),
  );
  const score = question.keywords.length
    ? Math.round((question.score * matchedCount) / question.keywords.length)
    : normalizedUserAnswer === normalizedStandardAnswer
      ? question.score
      : 0;

  return {
    questionKey: question.questionKey,
    userAnswer,
    standardAnswer: question.standardAnswer,
    score,
    maxScore: question.score,
    correct: score === question.score,
    missingKeywords,
    comment: question.keywords.length
      ? `匹配到 ${matchedCount}/${question.keywords.length} 个关键词`
      : "未配置关键词，按标准答案完全匹配评分",
  };
}

function collectQuizCandidates(stage: LearningAssistantQuizStageSnapshot): QuizCandidate[] {
  const candidates: QuizCandidate[] = [];
  const seen = new Set<string>();
  for (const entry of stage.learningEntries ?? []) {
    addCandidate(candidates, seen, {
      title: entry.title,
      section: entry.section,
      action: entry.studyAction || entry.practiceAction || entry.expectedOutput,
      checkMethod: entry.checkMethod,
      reason: entry.reason,
      sourceFile: entry.sourceFile,
    });
  }
  for (const point of stage.knowledgePoints ?? []) {
    addCandidate(candidates, seen, { title: point, section: clean(stage.name) });
  }
  for (const task of [
    ...(stage.learningTasks ?? []),
    ...(stage.practiceTasks ?? []),
    ...(stage.checkTasks ?? []),
  ]) {
    addCandidate(candidates, seen, { title: task, section: clean(stage.name) });
  }
  return candidates;
}

function addCandidate(
  candidates: QuizCandidate[],
  seen: Set<string>,
  candidate: Partial<QuizCandidate>,
) {
  const title = clean(candidate.title);
  if (!title) return;
  const key = title.toLocaleLowerCase();
  if (seen.has(key)) return;
  seen.add(key);
  candidates.push({
    title,
    section: clean(candidate.section),
    action: clean(candidate.action),
    checkMethod: clean(candidate.checkMethod),
    reason: clean(candidate.reason),
    sourceFile: clean(candidate.sourceFile),
  });
}

function buildQuestionText(
  type: LearningAssistantQuizQuestionType,
  candidate: QuizCandidate,
  stageName: string,
): string {
  if (type === "choice") {
    return `关于「${candidate.title}」，下列哪项最适合作为「${stageName}」的学习重点？`;
  }
  if (type === "judgment") {
    return `学习「${candidate.title}」时，应结合本阶段目标完成练习、检查和复盘。`;
  }
  return `简述「${candidate.title}」在「${stageName}」中的学习要点。`;
}

function buildOptions(
  type: LearningAssistantQuizQuestionType,
  candidate: QuizCandidate,
  stageGoal?: string,
): string[] {
  if (type === "judgment") return ["正确", "错误"];
  if (type === "short_answer") return [];
  return [
    buildCorrectOption(candidate, stageGoal),
    "跳过概念理解，只记录题目答案",
    "只背诵孤立术语，不结合工艺流程",
    "忽略练习反馈，直接进入下一阶段",
  ];
}

function buildStandardAnswer(
  type: LearningAssistantQuizQuestionType,
  candidate: QuizCandidate,
  stageGoal?: string,
): string {
  if (type === "judgment") return "正确";
  if (type === "choice") return buildCorrectOption(candidate, stageGoal);
  return [
    candidate.title,
    candidate.section ? `所属模块：${candidate.section}` : "",
    candidate.action ? `学习动作：${candidate.action}` : "",
    candidate.checkMethod ? `检查方式：${candidate.checkMethod}` : "",
    candidate.reason ? `依据：${candidate.reason}` : "",
  ]
    .filter(Boolean)
    .join("；");
}

function buildCorrectOption(candidate: QuizCandidate, stageGoal?: string): string {
  const action = candidate.action || candidate.checkMethod || stageGoal || "结合概念、工艺任务和检查标准完成学习";
  return `围绕「${candidate.title}」理解概念，并通过「${action}」验证掌握情况`;
}

function buildExplanation(
  type: LearningAssistantQuizQuestionType,
  candidate: QuizCandidate,
  stageName: string,
): string {
  if (type === "short_answer") {
    return `本题按关键词给分，重点看是否覆盖「${buildKeywords(candidate).join("、")}」。`;
  }
  return `题目来源于当前项目「${stageName}」的本地 fallback 计划条目，不调用正式题库。`;
}

function buildKeywords(candidate: QuizCandidate): string[] {
  return uniqueStrings([
    candidate.title,
    candidate.section,
    ...splitMeaningfulTerms(candidate.title),
    ...splitMeaningfulTerms(candidate.section),
  ]).slice(0, 5);
}

function questionTypeForIndex(index: number): LearningAssistantQuizQuestionType {
  if (index % 3 === 0) return "choice";
  if (index % 3 === 1) return "judgment";
  return "short_answer";
}

function difficultyForLevel(
  currentLevel: string | undefined,
  index: number,
): LearningAssistantQuizDifficulty {
  const level = currentLevel ?? "";
  if (/零基础|基础较弱|掌握不牢|beginner|weak/i.test(level)) return index > 3 ? "medium" : "easy";
  if (/基础较好|查漏补缺|提高|advanced/i.test(level)) return index > 2 ? "hard" : "medium";
  return index > 3 ? "hard" : "medium";
}

function getLearningQuizLevel(percentage: number): Pick<
  LearningAssistantQuizScoreResult,
  "level" | "feedback"
> {
  if (percentage >= 85) {
    return {
      level: "优秀",
      feedback: "当前阶段掌握较好，可以继续后续学习，并补充综合案例训练。",
    };
  }
  if (percentage >= 60) {
    return {
      level: "基本掌握",
      feedback: "已达到继续学习的基本要求，建议先复习薄弱知识点再进入下一阶段。",
    };
  }
  return {
    level: "需要重学",
    feedback: "当前阶段测试结果偏低，建议重新学习薄弱知识点并完成同类练习。",
  };
}

function buildLearningQuizSuggestions(percentage: number, weakKnowledgePoints: string[]): string[] {
  const weakText = weakKnowledgePoints.length
    ? weakKnowledgePoints.slice(0, 5).join("、")
    : "本阶段核心知识点";
  if (percentage >= 85) return ["保持当前节奏，可增加综合工艺案例或应用题。"];
  if (percentage >= 60) return [`继续后续学习前，先复习：${weakText}。`];
  return [
    `建议回到计划中重新学习：${weakText}。`,
    "完成错题复盘后，再生成一次阶段测试确认掌握情况。",
  ];
}

function parseQuizRecord(value: unknown): LearningAssistantQuizRecord | null {
  if (!isRecord(value) || value.source !== LEARNING_ASSISTANT_QUIZ_SOURCE) return null;
  const recordKey = readString(value, "recordKey");
  const testedAt = readString(value, "testedAt");
  const stageName = readString(value, "stageName");
  const stageIndex = readNonNegativeInteger(value.stageIndex);
  const totalScore = readNonNegativeNumber(value.totalScore);
  const maxScore = readNonNegativeNumber(value.maxScore);
  const percentage = readPercentage(value.percentage);
  const level = readLevel(value.level);
  if (
    !recordKey ||
    !testedAt ||
    !stageName ||
    stageIndex === null ||
    totalScore === null ||
    maxScore === null ||
    percentage === null ||
    !level
  ) {
    return null;
  }
  return {
    recordKey,
    source: LEARNING_ASSISTANT_QUIZ_SOURCE,
    testedAt,
    stageIndex,
    stageName,
    totalScore,
    maxScore,
    percentage,
    level,
    weakKnowledgePoints: readStringArray(value.weakKnowledgePoints),
    missingKeywords: readStringArray(value.missingKeywords),
    feedback: readString(value, "feedback"),
    suggestions: readStringArray(value.suggestions),
    canAdvance: value.canAdvance === true,
    items: Array.isArray(value.items)
      ? value.items
          .map(parseQuizRecordItem)
          .filter((item): item is LearningAssistantQuizRecordItem => item !== null)
      : [],
  };
}

function parseQuizRecordItem(value: unknown): LearningAssistantQuizRecordItem | null {
  if (!isRecord(value)) return null;
  const questionKey = readString(value, "questionKey");
  const question = readString(value, "question");
  const questionType = readQuestionType(value.questionType);
  const standardAnswer = readString(value, "standardAnswer");
  const maxScore = readNonNegativeNumber(value.maxScore);
  const score = readNonNegativeNumber(value.score);
  const knowledgePoint = readString(value, "knowledgePoint");
  if (
    !questionKey ||
    !question ||
    !questionType ||
    !standardAnswer ||
    maxScore === null ||
    score === null ||
    !knowledgePoint ||
    score > maxScore
  ) {
    return null;
  }
  return {
    questionKey,
    question,
    questionType,
    options: readStringArray(value.options),
    userAnswer: readString(value, "userAnswer"),
    standardAnswer,
    explanation: readString(value, "explanation"),
    score,
    maxScore,
    correct: value.correct === true,
    knowledgePoint,
    missingKeywords: readStringArray(value.missingKeywords),
  };
}

function normalizeQuizAnswer(answer: string): string {
  return answer.trim().toLocaleLowerCase().replace(/\s+/g, "");
}

function splitMeaningfulTerms(value: string | undefined): string[] {
  const text = clean(value);
  if (!text) return [];
  return text
    .split(/[、，,；;：:\s/()-]+/)
    .map((item) => item.trim())
    .filter((item) => item.length >= 2);
}

function makeQuestionKey(stageIndex: number, title: string, index: number): string {
  return `quiz-${stageIndex + 1}-${index + 1}-${hashText(title)}`;
}

function makeRecordKey(stageIndex: number, testedAt: string): string {
  return `quiz-record-${stageIndex + 1}-${testedAt.replace(/[^0-9A-Za-z]/g, "").slice(0, 20)}`;
}

function hashText(value: string): string {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

function clean(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function uniqueStrings(values: Array<string | undefined>): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const text = clean(value);
    if (!text) continue;
    const key = text.toLocaleLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(text);
  }
  return result;
}

function clampInteger(value: number, min: number, max: number): number {
  if (!Number.isSafeInteger(value)) return min;
  return Math.min(max, Math.max(min, value));
}

function readString(source: Record<string, unknown>, key: string): string {
  return clean(source[key]);
}

function readStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter(isNonEmptyString).slice(0, 30) : [];
}

function readQuestionType(value: unknown): LearningAssistantQuizQuestionType | null {
  return value === "choice" || value === "judgment" || value === "short_answer"
    ? value
    : null;
}

function readLevel(value: unknown): LearningAssistantQuizScoreResult["level"] | null {
  return value === "优秀" || value === "基本掌握" || value === "需要重学" ? value : null;
}

function readNonNegativeInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function readNonNegativeNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}

function readPercentage(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 100
    ? value
    : null;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
