export const LEARNING_ASSISTANT_DIAGNOSIS_SOURCE = "local-fallback-diagnosis";

export interface LearningAssistantGoalSnapshot {
  courseName: string;
  learningGoal: string;
  learningCycle: string;
  dailyStudyHours: number;
  currentLevel: string;
  finalGoal: string;
}

export interface LearningAssistantPlanEntrySnapshot {
  title: string;
  section?: string;
  masteryLevel?: string;
  reason?: string;
  sourceFile?: string;
  sourceType?: string;
}

export interface LearningAssistantStageSnapshot {
  knowledgePoints?: string[];
  learningEntries?: LearningAssistantPlanEntrySnapshot[];
}

export interface LearningAssistantPlanSnapshot {
  stages?: LearningAssistantStageSnapshot[];
}

export interface LearningAssistantDiagnosis {
  source: typeof LEARNING_ASSISTANT_DIAGNOSIS_SOURCE;
  generatedAt: string;
  basis: string[];
  summary: string;
  masteredKnowledgePoints: string[];
  pendingKnowledgePoints: string[];
  weakKnowledgePoints: string[];
  suggestions: string[];
  knowledgePointCount: number;
}

interface KnowledgeCandidate {
  title: string;
  masteryLevel?: string;
  sourceType?: string;
}

export function buildLearningAssistantDiagnosis(
  goal: LearningAssistantGoalSnapshot,
  plan?: LearningAssistantPlanSnapshot | null,
  generatedAt = new Date().toISOString(),
): LearningAssistantDiagnosis {
  const candidates = collectPlanKnowledgeCandidates(plan);
  const pendingKnowledgePoints = candidates.map((item) => item.title).slice(0, 18);
  const masteredCount = inferMasteredCount(goal.currentLevel, candidates.length);
  const weakCount = inferWeakCount(goal.currentLevel, goal.learningGoal, candidates.length);
  const masteredKnowledgePoints = pendingKnowledgePoints.slice(0, masteredCount);
  const weakKnowledgePoints = pendingKnowledgePoints
    .filter((point) => !masteredKnowledgePoints.includes(point))
    .slice(0, weakCount);
  const basis = [
    "学习目标",
    "当前基础自评",
    plan?.stages?.length ? "本地知识点计划条目" : "待生成计划后补充知识点证据",
  ];

  return {
    source: LEARNING_ASSISTANT_DIAGNOSIS_SOURCE,
    generatedAt,
    basis,
    summary: buildDiagnosisSummary(goal, masteredKnowledgePoints, weakKnowledgePoints),
    masteredKnowledgePoints,
    pendingKnowledgePoints,
    weakKnowledgePoints,
    suggestions: buildDiagnosisSuggestions(goal, weakKnowledgePoints),
    knowledgePointCount: pendingKnowledgePoints.length,
  };
}

export function attachDiagnosisToUnderstanding<T extends Record<string, unknown>>(
  understanding: T,
  diagnosis: LearningAssistantDiagnosis,
): T & {
  diagnosis: LearningAssistantDiagnosis;
  masteredKnowledgePoints: string[];
  pendingKnowledgePoints: string[];
  weakKnowledgePoints: string[];
} {
  return {
    ...understanding,
    diagnosis,
    masteredKnowledgePoints: diagnosis.masteredKnowledgePoints,
    pendingKnowledgePoints: diagnosis.pendingKnowledgePoints,
    weakKnowledgePoints: diagnosis.weakKnowledgePoints,
  };
}

export function extractLearningAssistantDiagnosis(
  value: unknown,
): LearningAssistantDiagnosis | null {
  if (!isRecord(value)) return null;
  const raw = value.diagnosis;
  if (!isRecord(raw)) return null;
  if (raw.source !== LEARNING_ASSISTANT_DIAGNOSIS_SOURCE) return null;
  if (typeof raw.generatedAt !== "string" || !raw.generatedAt.trim()) return null;
  if (typeof raw.summary !== "string" || !raw.summary.trim()) return null;
  const basis = readStringArray(raw.basis);
  const masteredKnowledgePoints = readStringArray(raw.masteredKnowledgePoints);
  const pendingKnowledgePoints = readStringArray(raw.pendingKnowledgePoints);
  const weakKnowledgePoints = readStringArray(raw.weakKnowledgePoints);
  const suggestions = readStringArray(raw.suggestions);
  const knowledgePointCount =
    typeof raw.knowledgePointCount === "number" &&
    Number.isSafeInteger(raw.knowledgePointCount) &&
    raw.knowledgePointCount >= 0
      ? raw.knowledgePointCount
      : pendingKnowledgePoints.length;

  return {
    source: LEARNING_ASSISTANT_DIAGNOSIS_SOURCE,
    generatedAt: raw.generatedAt,
    basis,
    summary: raw.summary,
    masteredKnowledgePoints,
    pendingKnowledgePoints,
    weakKnowledgePoints,
    suggestions,
    knowledgePointCount,
  };
}

function collectPlanKnowledgeCandidates(
  plan?: LearningAssistantPlanSnapshot | null,
): KnowledgeCandidate[] {
  const seen = new Set<string>();
  const candidates: KnowledgeCandidate[] = [];
  for (const stage of plan?.stages ?? []) {
    for (const entry of stage.learningEntries ?? []) {
      addCandidate(candidates, seen, {
        title: entry.title,
        masteryLevel: entry.masteryLevel,
        sourceType: entry.sourceType,
      });
    }
    for (const point of stage.knowledgePoints ?? []) {
      addCandidate(candidates, seen, { title: point, sourceType: "knowledgeBase" });
    }
  }
  return candidates;
}

function addCandidate(
  candidates: KnowledgeCandidate[],
  seen: Set<string>,
  candidate: KnowledgeCandidate,
) {
  const title = clean(candidate.title);
  if (!title) return;
  const key = title.toLocaleLowerCase();
  if (seen.has(key)) return;
  seen.add(key);
  candidates.push({ ...candidate, title });
}

function inferMasteredCount(currentLevel: string, total: number): number {
  if (total <= 0) return 0;
  if (/零基础|基本没有/.test(currentLevel)) return 0;
  if (/基础较弱|掌握不牢/.test(currentLevel)) return Math.min(1, total);
  if (/基础较好|查漏补缺/.test(currentLevel)) return Math.min(4, total);
  return Math.min(2, total);
}

function inferWeakCount(currentLevel: string, learningGoal: string, total: number): number {
  if (total <= 0) return 0;
  if (/零基础|基本没有/.test(currentLevel)) return Math.min(6, total);
  if (/基础较弱|掌握不牢/.test(currentLevel)) return Math.min(5, total);
  if (/查漏补缺|期末冲刺/.test(learningGoal)) return Math.min(4, total);
  return Math.min(3, total);
}

function buildDiagnosisSummary(
  goal: LearningAssistantGoalSnapshot,
  masteredKnowledgePoints: string[],
  weakKnowledgePoints: string[],
): string {
  const masteredText = masteredKnowledgePoints.length
    ? `已具备 ${masteredKnowledgePoints.length} 个可继续利用的知识点基础`
    : "当前先按待诊断基础处理";
  const weakText = weakKnowledgePoints.length
    ? `优先补齐 ${weakKnowledgePoints.length} 个薄弱知识点`
    : "生成计划后继续识别薄弱知识点";
  return `${goal.courseName} · ${goal.learningGoal}：${masteredText}，${weakText}。`;
}

function buildDiagnosisSuggestions(
  goal: LearningAssistantGoalSnapshot,
  weakKnowledgePoints: string[],
): string[] {
  const weakText = weakKnowledgePoints.length
    ? weakKnowledgePoints.slice(0, 3).join("、")
    : "本阶段核心知识点";
  return [
    `每天保持 ${formatHours(goal.dailyStudyHours)} 的固定学习节奏。`,
    `围绕「${weakText}」先补概念，再做练习和复述。`,
    `每完成一个阶段后用小测验校验掌握度，再决定是否重学或推进。`,
  ];
}

function formatHours(hours: number): string {
  if (!Number.isFinite(hours) || hours <= 0) return "1 小时";
  return `${hours} 小时`;
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && item.trim().length > 0);
}

function clean(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
