export const DEFAULT_PPT_RESERVED_OUTPUT_TOKENS = 8192;
export const PPT_CONTEXT_NEAR_LIMIT_RATIO = 0.8;

export type PptContextBudgetStatus = "safe" | "near_limit" | "exceeded" | "unknown";

export interface PptContextBudgetInput {
  modelMaxContextTokens?: number | null;
  rawMaterial: string;
  promptText: string;
  metadataText?: string;
  reservedOutputTokens?: number;
}

export interface PptContextBudgetResult {
  maxContextTokens: number | null;
  estimatedMaterialTokens: number;
  estimatedPromptTokens: number;
  estimatedMetadataTokens: number;
  estimatedInputTokens: number;
  reservedOutputTokens: number;
  effectiveInputBudget: number | null;
  remainingTokens: number | null;
  usageRatio: number | null;
  status: PptContextBudgetStatus;
}

const URL_PATTERN = /(?:https?:\/\/|www\.)[^\s<>{}\[\]"']+/giu;
const CJK_PATTERN = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;
const LETTER_PATTERN = /[\p{Letter}\p{Mark}]/u;
const DIGIT_PATTERN = /\p{Number}/u;
const WHITESPACE_PATTERN = /\s/u;
const PUNCTUATION_PATTERN = /[\p{Punctuation}\p{Symbol}]/u;

type TextCategory = "cjk" | "letter" | "digit" | "whitespace" | "punctuation" | "other";

function characterCategory(character: string): TextCategory {
  if (CJK_PATTERN.test(character)) return "cjk";
  if (LETTER_PATTERN.test(character)) return "letter";
  if (DIGIT_PATTERN.test(character)) return "digit";
  if (WHITESPACE_PATTERN.test(character)) return "whitespace";
  if (PUNCTUATION_PATTERN.test(character)) return "punctuation";
  return "other";
}

function estimateRunTokens(category: TextCategory, value: string): number {
  const length = Array.from(value).length;
  switch (category) {
    case "cjk":
      // 不同模型对 CJK 的切分差异较大，按每字 1.2 token 做保守估算。
      return length * 1.2;
    case "letter":
      // 英文及其他拼音文字通常约 3-4 个字符一个 token，短单词至少计一个。
      return Math.max(1, Math.ceil(length / 4));
    case "digit":
      return Math.max(1, Math.ceil(length / 3));
    case "whitespace": {
      const lineBreaks = (value.match(/\r?\n/g) ?? []).length;
      const remaining = Math.max(0, length - lineBreaks);
      return Math.ceil(lineBreaks / 3) + Math.ceil(remaining / 16);
    }
    case "punctuation":
      // Markdown 标记、标点和常见符号通常会形成独立或半独立 token。
      return Math.ceil(length * 0.9);
    case "other":
      // emoji、罕见符号等按最多两个 token 估算，避免把专业符号低估为零。
      return length * 2;
  }
}

/**
 * 轻量、与具体模型 tokenizer 无关的预计 token 数。
 *
 * 该函数只用于容量提示和安全分块，不把结果伪装成服务端精确计数。
 */
export function estimatePptTextTokens(text: string): number {
  if (!text) return 0;

  let estimated = 0;
  const textWithoutUrls = text.replace(URL_PATTERN, (url) => {
    // URL 中的斜杠、参数和编码片段切分较碎，按约 3 字符/token 并增加边界开销。
    estimated += Math.ceil(Array.from(url).length / 3) + 2;
    return " ";
  });

  let currentCategory: TextCategory | null = null;
  let currentRun = "";
  const flush = () => {
    if (currentCategory && currentRun) {
      estimated += estimateRunTokens(currentCategory, currentRun);
    }
    currentCategory = null;
    currentRun = "";
  };

  for (const character of textWithoutUrls) {
    const category = characterCategory(character);
    // CJK 字符逐字计费；拼音文字、数字、标点和空白则按连续片段估算。
    if (category === "cjk") {
      flush();
      estimated += estimateRunTokens(category, character);
      continue;
    }
    if (currentCategory !== category) {
      flush();
      currentCategory = category;
    }
    currentRun += character;
  }
  flush();

  return Math.ceil(estimated);
}

export function resolvePptReservedOutputTokens(modelMaxOutputTokens?: number | null): number {
  if (Number.isFinite(modelMaxOutputTokens) && (modelMaxOutputTokens ?? 0) > 0) {
    return Math.floor(modelMaxOutputTokens as number);
  }
  return DEFAULT_PPT_RESERVED_OUTPUT_TOKENS;
}

export function calculatePptContextBudget(input: PptContextBudgetInput): PptContextBudgetResult {
  const maxContextTokens = Number.isFinite(input.modelMaxContextTokens) && (input.modelMaxContextTokens ?? 0) > 0
    ? Math.floor(input.modelMaxContextTokens as number)
    : null;
  const reservedOutputTokens = resolvePptReservedOutputTokens(input.reservedOutputTokens);
  const estimatedMaterialTokens = estimatePptTextTokens(input.rawMaterial);
  const estimatedPromptTokens = estimatePptTextTokens(input.promptText);
  const estimatedMetadataTokens = estimatePptTextTokens(input.metadataText ?? "");
  const estimatedInputTokens =
    estimatedMaterialTokens + estimatedPromptTokens + estimatedMetadataTokens;

  if (maxContextTokens === null) {
    return {
      maxContextTokens,
      estimatedMaterialTokens,
      estimatedPromptTokens,
      estimatedMetadataTokens,
      estimatedInputTokens,
      reservedOutputTokens,
      effectiveInputBudget: null,
      remainingTokens: null,
      usageRatio: null,
      status: "unknown",
    };
  }

  const effectiveInputBudget = Math.max(0, maxContextTokens - reservedOutputTokens);
  const remainingTokens = effectiveInputBudget - estimatedInputTokens;
  const usageRatio = effectiveInputBudget === 0
    ? estimatedInputTokens > 0
      ? Number.POSITIVE_INFINITY
      : 0
    : estimatedInputTokens / effectiveInputBudget;
  const status: PptContextBudgetStatus = estimatedInputTokens > effectiveInputBudget
    ? "exceeded"
    : usageRatio >= PPT_CONTEXT_NEAR_LIMIT_RATIO
      ? "near_limit"
      : "safe";

  return {
    maxContextTokens,
    estimatedMaterialTokens,
    estimatedPromptTokens,
    estimatedMetadataTokens,
    estimatedInputTokens,
    reservedOutputTokens,
    effectiveInputBudget,
    remainingTokens,
    usageRatio,
    status,
  };
}
