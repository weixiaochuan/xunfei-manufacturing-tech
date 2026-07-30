import type { JsonArray, JsonObject } from "./accountLearningTypes.ts";

export type LearningAssistantStageProgressStatus =
  | "completed"
  | "needsReview"
  | "inProgress"
  | "notStarted";

export interface LearningAssistantStageProgress {
  stageIndex: number;
  stageName: string;
  status: LearningAssistantStageProgressStatus;
  latestPercentage: number | null;
  latestTestedAt: string | null;
  weakKnowledgePoints: string[];
}

export interface LearningAssistantProgressActivity {
  activityKey: string;
  activityType: "project" | "qa" | "quiz" | "replan" | "document" | "resource";
  message: string;
  occurredAt: string;
}

export interface LearningAssistantProgressOverview {
  stageCount: number;
  completedStageCount: number;
  needsReviewStageCount: number;
  progressPercent: number;
  quizRecordCount: number;
  qaRecordCount: number;
  linkedDocumentCount: number;
  mastery: {
    mastered: number;
    basic: number;
    weak: number;
  };
  latestActivityAt: string | null;
  stageStatuses: LearningAssistantStageProgress[];
  recentActivities: LearningAssistantProgressActivity[];
}

interface BuildProgressOverviewInput {
  plan: unknown;
  progress: JsonObject | null | undefined;
  linkedDocumentCount: number;
  planAdjustments?: JsonArray | null;
}

interface StageSnapshot {
  name: string;
}

interface QuizSnapshot {
  recordKey: string;
  stageIndex: number;
  stageName: string;
  percentage: number;
  testedAt: string;
  canAdvance: boolean;
  weakKnowledgePoints: string[];
}

interface MasterySnapshot {
  masteryLevel: "mastered" | "basic" | "weak";
}

export function appendLearningAssistantActivityToProgress(
  progress: JsonObject | null | undefined,
  activity: LearningAssistantProgressActivity,
  limit = 20,
): JsonObject {
  const previous = isRecord(progress) ? progress : {};
  const parsed = parseStoredActivity(activity);
  if (!parsed) return { ...previous };
  const existing = readStoredActivities(previous).filter(
    (item) => item.activityKey !== parsed.activityKey,
  );
  const records = [parsed, ...existing]
    .sort((left, right) => right.occurredAt.localeCompare(left.occurredAt))
    .slice(0, Math.max(1, Math.min(limit, 50)));
  return {
    ...previous,
    activityRecords: records,
    activityRecordCount: records.length,
    latestActivityAt: records[0]?.occurredAt ?? parsed.occurredAt,
  };
}

export function buildLearningAssistantProgressOverview(
  input: BuildProgressOverviewInput,
): LearningAssistantProgressOverview {
  const progress = isRecord(input.progress) ? input.progress : {};
  const stages = readStages(input.plan);
  const quizzes = readQuizRecords(progress);
  const masteryRecords = readMasteryRecords(progress);
  const activities = buildRecentActivities({
    progress,
    quizzes,
    planAdjustments: input.planAdjustments,
    linkedDocumentCount: input.linkedDocumentCount,
  });
  const stageStatuses = buildStageStatuses(stages, quizzes);
  const completedStageCount = stageStatuses.filter((stage) => stage.status === "completed").length;
  const needsReviewStageCount = stageStatuses.filter((stage) => stage.status === "needsReview").length;

  return {
    stageCount: stages.length,
    completedStageCount,
    needsReviewStageCount,
    progressPercent: stages.length ? Math.round((completedStageCount / stages.length) * 100) : 0,
    quizRecordCount: quizzes.length,
    qaRecordCount: readArray(progress.qaRecords).length,
    linkedDocumentCount: normalizeCount(input.linkedDocumentCount),
    mastery: {
      mastered: masteryRecords.filter((record) => record.masteryLevel === "mastered").length,
      basic: masteryRecords.filter((record) => record.masteryLevel === "basic").length,
      weak: masteryRecords.filter((record) => record.masteryLevel === "weak").length,
    },
    latestActivityAt: activities[0]?.occurredAt ?? null,
    stageStatuses,
    recentActivities: activities,
  };
}

function buildStageStatuses(
  stages: StageSnapshot[],
  quizzes: QuizSnapshot[],
): LearningAssistantStageProgress[] {
  let firstOpenStageFound = false;
  return stages.map((stage, index) => {
    const latestQuiz = latestQuizForStage(quizzes, index);
    if (latestQuiz) {
      return {
        stageIndex: index,
        stageName: stage.name,
        status: latestQuiz.canAdvance ? "completed" : "needsReview",
        latestPercentage: latestQuiz.percentage,
        latestTestedAt: latestQuiz.testedAt,
        weakKnowledgePoints: latestQuiz.weakKnowledgePoints,
      };
    }

    const status: LearningAssistantStageProgressStatus = firstOpenStageFound
      ? "notStarted"
      : "inProgress";
    firstOpenStageFound = true;
    return {
      stageIndex: index,
      stageName: stage.name,
      status,
      latestPercentage: null,
      latestTestedAt: null,
      weakKnowledgePoints: [],
    };
  });
}

function buildRecentActivities(input: {
  progress: JsonObject;
  quizzes: QuizSnapshot[];
  planAdjustments?: JsonArray | null;
  linkedDocumentCount: number;
}): LearningAssistantProgressActivity[] {
  const activities: LearningAssistantProgressActivity[] = [];
  activities.push(...readStoredActivities(input.progress));

  const updatedAt = readOptionalString(input.progress.updatedAt);
  if (updatedAt) {
    activities.push({
      activityKey: `project-${updatedAt}`,
      activityType: "project",
      message: "项目进度已更新",
      occurredAt: updatedAt,
    });
  }

  for (const quiz of input.quizzes) {
    activities.push({
      activityKey: `quiz-${quiz.recordKey}`,
      activityType: "quiz",
      message: `${quiz.stageName} 阶段测试 ${quiz.percentage}%`,
      occurredAt: quiz.testedAt,
    });
  }

  for (const record of readArray(input.progress.qaRecords)) {
    if (!isRecord(record)) continue;
    const askedAt = readOptionalString(record.askedAt);
    const question = readOptionalString(record.question);
    if (!askedAt || !question) continue;
    activities.push({
      activityKey: `qa-${hashText(question)}-${askedAt}`,
      activityType: "qa",
      message: `知识库问答：${question}`,
      occurredAt: askedAt,
    });
  }

  for (const adjustment of readArray(input.planAdjustments)) {
    if (!isRecord(adjustment)) continue;
    const adjustedAt = readOptionalString(adjustment.adjustedAt);
    const reason = readOptionalString(adjustment.reason);
    if (!adjustedAt) continue;
    activities.push({
      activityKey: `replan-${hashText(reason ?? adjustedAt)}-${adjustedAt}`,
      activityType: "replan",
      message: reason || "学习计划已根据测试结果调整",
      occurredAt: adjustedAt,
    });
  }

  if (normalizeCount(input.linkedDocumentCount) > 0) {
    activities.push({
      activityKey: `document-${input.linkedDocumentCount}`,
      activityType: "document",
      message: `已关联 ${normalizeCount(input.linkedDocumentCount)} 份项目资料`,
      occurredAt: updatedAt ?? new Date(0).toISOString(),
    });
  }

  return activities
    .filter((activity) => isIsoLikeString(activity.occurredAt))
    .sort((left, right) => right.occurredAt.localeCompare(left.occurredAt))
    .slice(0, 8);
}

function readStages(plan: unknown): StageSnapshot[] {
  if (!isRecord(plan) || !Array.isArray(plan.stages)) return [];
  return plan.stages
    .map((stage, index) => {
      if (!isRecord(stage)) return null;
      return {
        name: readOptionalString(stage.name) || `阶段 ${index + 1}`,
      };
    })
    .filter((stage): stage is StageSnapshot => Boolean(stage));
}

function readQuizRecords(progress: JsonObject): QuizSnapshot[] {
  return readArray(progress.quizRecords)
    .map((record) => {
      if (!isRecord(record)) return null;
      const recordKey = readOptionalString(record.recordKey);
      const stageIndex = readSafeInteger(record.stageIndex);
      const stageName = readOptionalString(record.stageName);
      const percentage = readPercentage(record.percentage);
      const testedAt = readOptionalString(record.testedAt);
      if (!recordKey || stageIndex === null || !stageName || percentage === null || !testedAt) {
        return null;
      }
      return {
        recordKey,
        stageIndex,
        stageName,
        percentage,
        testedAt,
        canAdvance: record.canAdvance === true,
        weakKnowledgePoints: readStringArray(record.weakKnowledgePoints),
      };
    })
    .filter((record): record is QuizSnapshot => Boolean(record));
}

function readMasteryRecords(progress: JsonObject): MasterySnapshot[] {
  return readArray(progress.masteryRecords)
    .map((record) => {
      if (!isRecord(record)) return null;
      const masteryLevel = record.masteryLevel;
      if (masteryLevel !== "mastered" && masteryLevel !== "basic" && masteryLevel !== "weak") {
        return null;
      }
      return { masteryLevel };
    })
    .filter((record): record is MasterySnapshot => Boolean(record));
}

function readStoredActivities(progress: JsonObject): LearningAssistantProgressActivity[] {
  return readArray(progress.activityRecords)
    .map(parseStoredActivity)
    .filter((record): record is LearningAssistantProgressActivity => Boolean(record));
}

function parseStoredActivity(value: unknown): LearningAssistantProgressActivity | null {
  if (!isRecord(value)) return null;
  const activityKey = readOptionalString(value.activityKey);
  const message = readOptionalString(value.message);
  const occurredAt = readOptionalString(value.occurredAt);
  const activityType = readActivityType(value.activityType);
  if (!activityKey || !message || !occurredAt || !activityType || !isIsoLikeString(occurredAt)) {
    return null;
  }
  return {
    activityKey: activityKey.slice(0, 160),
    activityType,
    message: message.slice(0, 240),
    occurredAt,
  };
}

function readActivityType(value: unknown): LearningAssistantProgressActivity["activityType"] | null {
  return value === "project" ||
    value === "qa" ||
    value === "quiz" ||
    value === "replan" ||
    value === "document" ||
    value === "resource"
    ? value
    : null;
}

function latestQuizForStage(quizzes: QuizSnapshot[], stageIndex: number): QuizSnapshot | null {
  return (
    quizzes
      .filter((quiz) => quiz.stageIndex === stageIndex)
      .sort((left, right) => right.testedAt.localeCompare(left.testedAt))[0] ?? null
  );
}

function readArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function readOptionalString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function readStringArray(value: unknown): string[] {
  return readArray(value)
    .filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    .map((item) => item.trim())
    .slice(0, 20);
}

function readSafeInteger(value: unknown): number | null {
  return Number.isSafeInteger(value) && Number(value) >= 0 ? Number(value) : null;
}

function readPercentage(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  if (value < 0 || value > 100) return null;
  return Math.round(value);
}

function normalizeCount(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 ? value : 0;
}

function isIsoLikeString(value: string): boolean {
  return /\d{4}-\d{2}-\d{2}/.test(value);
}

function hashText(value: string): string {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash.toString(36);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
