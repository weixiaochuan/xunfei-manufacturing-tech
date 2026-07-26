import type { JsonObject, LearningProjectDocument } from "./accountLearningTypes.ts";

export const LEARNING_ASSISTANT_QA_SOURCE = "local-kb-extractive-qa";
export const LEARNING_ASSISTANT_QA_UNAVAILABLE_SOURCE = "local-kb-unavailable";
export const LEARNING_ASSISTANT_QA_RECORD_LIMIT = 30;

export interface LearningAssistantKbResultItem {
  sourceFile: string;
  sheetName: string;
  section: string;
  title: string;
  content: string;
  matchedKeywords: string[];
  score: number;
  reason: string;
}

export interface LearningAssistantKbSearchResult {
  results: LearningAssistantKbResultItem[];
  message: string;
  warnings?: string[];
}

export interface LearningAssistantQaSource {
  sourceKey: string;
  sourceKind: "knowledgeBase" | "projectDocument";
  title: string;
  sourceFile?: string;
  sheetName?: string;
  section?: string;
  matchedKeywords: string[];
  confidence: number;
  status?: "available" | "deleted";
}

export interface LearningAssistantQaRecord {
  recordKey: string;
  question: string;
  answer: string;
  askedAt: string;
  sources: LearningAssistantQaSource[];
  confidence: number;
  generationType:
    | typeof LEARNING_ASSISTANT_QA_SOURCE
    | typeof LEARNING_ASSISTANT_QA_UNAVAILABLE_SOURCE;
}

export interface BuildLearningAssistantQaInput {
  question: string;
  searched: LearningAssistantKbSearchResult;
  documents?: LearningProjectDocument[];
  askedAt?: string;
}

export function buildLearningAssistantQaRecord(
  input: BuildLearningAssistantQaInput,
): LearningAssistantQaRecord {
  const askedAt = input.askedAt ?? new Date().toISOString();
  const question = input.question.trim();
  const knowledgeSources = input.searched.results
    .filter(isSafeKbResult)
    .slice(0, 4)
    .map(sourceFromKbResult);
  const documentSources = (input.documents ?? [])
    .filter((document) => document.status === "available")
    .slice(0, 3)
    .map(sourceFromDocument);
  const sources = [...knowledgeSources, ...documentSources];
  const confidence = knowledgeSources.length
    ? roundConfidence(
        knowledgeSources.reduce((sum, source) => sum + source.confidence, 0) /
          knowledgeSources.length,
      )
    : 0;

  return {
    recordKey: makeRecordKey(askedAt, question),
    question,
    answer: knowledgeSources.length
      ? buildExtractiveAnswer(question, input.searched.results.slice(0, knowledgeSources.length))
      : buildUnavailableAnswer(input.searched.message, documentSources.length),
    askedAt,
    sources,
    confidence,
    generationType: knowledgeSources.length
      ? LEARNING_ASSISTANT_QA_SOURCE
      : LEARNING_ASSISTANT_QA_UNAVAILABLE_SOURCE,
  };
}

export function appendLearningAssistantQaRecordToProgress(
  progress: JsonObject | null | undefined,
  record: LearningAssistantQaRecord,
  limit = LEARNING_ASSISTANT_QA_RECORD_LIMIT,
): JsonObject {
  const previous = isRecord(progress) ? progress : {};
  const records = extractLearningAssistantQaRecords(previous);
  return {
    ...previous,
    qaRecords: [record, ...records.filter((item) => item.recordKey !== record.recordKey)].slice(0, limit),
    qaRecordCount: Math.min(records.length + 1, limit),
    latestQaAt: record.askedAt,
  };
}

export function extractLearningAssistantQaRecords(
  progress: JsonObject | null | undefined,
): LearningAssistantQaRecord[] {
  if (!isRecord(progress) || !Array.isArray(progress.qaRecords)) return [];
  return progress.qaRecords
    .map(parseQaRecord)
    .filter((record): record is LearningAssistantQaRecord => record !== null);
}

function buildExtractiveAnswer(question: string, results: LearningAssistantKbResultItem[]): string {
  const sections = results.slice(0, 3).map((item, index) => {
    const content = compactText(item.content, 180);
    const title = compactText(item.title, 60);
    return `${index + 1}. ${title}：${content}`;
  });
  return [
    `针对“${compactText(question, 80)}”，本地知识库检索到以下可引用内容：`,
    ...sections,
    "以上回答只基于本地知识点 Excel 命中内容和当前项目资料摘要；未调用真实模型，也未补造引用。",
  ].join("\n");
}

function buildUnavailableAnswer(message: string, documentSourceCount: number): string {
  const hint = documentSourceCount
    ? `当前项目有 ${documentSourceCount} 个可用关联资料，但第一版尚未解析资料正文，不能把它们冒充为答案依据。`
    : "当前项目暂无可用于补充判断的关联资料摘要。";
  return [
    `本地知识库暂未找到可引用内容。${message ? `检索反馈：${message}` : ""}`,
    hint,
    "请换用更具体的制造工艺学知识点继续提问，或先上传并关联学习资料。",
  ].join("\n");
}

function sourceFromKbResult(item: LearningAssistantKbResultItem): LearningAssistantQaSource {
  return {
    sourceKey: makeSourceKey(item),
    sourceKind: "knowledgeBase",
    title: item.title,
    sourceFile: item.sourceFile,
    sheetName: item.sheetName,
    section: item.section,
    matchedKeywords: item.matchedKeywords.filter(isNonEmptyString).slice(0, 8),
    confidence: scoreToConfidence(item.score),
  };
}

function sourceFromDocument(document: LearningProjectDocument): LearningAssistantQaSource {
  return {
    sourceKey: `document-${document.documentId}`,
    sourceKind: "projectDocument",
    title: document.title,
    matchedKeywords: [],
    confidence: document.importance === "core" ? 0.55 : document.importance === "important" ? 0.45 : 0.35,
    status: document.status,
  };
}

function parseQaRecord(value: unknown): LearningAssistantQaRecord | null {
  if (!isRecord(value)) return null;
  const recordKey = readString(value, "recordKey");
  const question = readString(value, "question");
  const answer = readString(value, "answer");
  const askedAt = readString(value, "askedAt");
  const generationType = readString(value, "generationType");
  if (
    !recordKey ||
    !question ||
    !answer ||
    !askedAt ||
    (generationType !== LEARNING_ASSISTANT_QA_SOURCE &&
      generationType !== LEARNING_ASSISTANT_QA_UNAVAILABLE_SOURCE)
  ) {
    return null;
  }
  const confidence = readConfidence(value.confidence);
  if (confidence === null) return null;
  const sources = Array.isArray(value.sources)
    ? value.sources
        .map(parseQaSource)
        .filter((source): source is LearningAssistantQaSource => source !== null)
    : [];
  return { recordKey, question, answer, askedAt, sources, confidence, generationType };
}

function parseQaSource(value: unknown): LearningAssistantQaSource | null {
  if (!isRecord(value)) return null;
  const sourceKey = readString(value, "sourceKey");
  const sourceKind = readString(value, "sourceKind");
  const title = readString(value, "title");
  if (!sourceKey || !title || (sourceKind !== "knowledgeBase" && sourceKind !== "projectDocument")) {
    return null;
  }
  const confidence = readConfidence(value.confidence);
  if (confidence === null) return null;
  return {
    sourceKey,
    sourceKind,
    title,
    sourceFile: readString(value, "sourceFile") || undefined,
    sheetName: readString(value, "sheetName") || undefined,
    section: readString(value, "section") || undefined,
    matchedKeywords: Array.isArray(value.matchedKeywords)
      ? value.matchedKeywords.filter(isNonEmptyString).slice(0, 8)
      : [],
    confidence,
    status: value.status === "available" || value.status === "deleted" ? value.status : undefined,
  };
}

function isSafeKbResult(value: LearningAssistantKbResultItem): boolean {
  return (
    isNonEmptyString(value.title) &&
    isNonEmptyString(value.content) &&
    isNonEmptyString(value.sourceFile) &&
    isNonEmptyString(value.sheetName) &&
    Number.isFinite(value.score)
  );
}

function makeRecordKey(askedAt: string, question: string): string {
  return `qa-${askedAt.replace(/[^0-9A-Za-z]/g, "").slice(0, 20)}-${hashText(question)}`;
}

function makeSourceKey(item: LearningAssistantKbResultItem): string {
  return `kb-${hashText(`${item.sourceFile}|${item.sheetName}|${item.section}|${item.title}`)}`;
}

function scoreToConfidence(score: number): number {
  if (!Number.isFinite(score) || score <= 0) return 0.25;
  return roundConfidence(Math.max(0.25, Math.min(0.95, score / 40)));
}

function roundConfidence(value: number): number {
  return Math.round(value * 100) / 100;
}

function hashText(value: string): string {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

function compactText(value: string, maxLength: number): string {
  const text = value.replace(/\s+/g, " ").trim();
  if (text.length <= maxLength) return text;
  return `${text.slice(0, maxLength - 1)}…`;
}

function readString(source: Record<string, unknown>, key: string): string {
  const value = source[key];
  return typeof value === "string" && value.trim() ? value.trim() : "";
}

function readConfidence(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1) {
    return null;
  }
  return value;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
