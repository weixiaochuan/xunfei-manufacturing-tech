import type { PptUnderstandingDraft } from "@/types";

export const PPT_NO_OPEN_QUESTIONS_TEXT =
  "暂无需要用户补充的信息，系统将根据现有材料自动完成内容组织与版式规划。";

const NUMBERED_ITEM_MARKER =
  /(?<![\d.])(?:\d{1,2}[.、](?=\s|[\p{L}“”"《（])|（\d{1,2}）(?=\s|[\p{L}“”"《（]))/gu;
const INTERNAL_READING_TERM = /第一块|第二块|分块|chunk|技术片段/iu;
const SYSTEM_PLANNING_QUESTION =
  /页面.{0,8}(?:排版|布局|安排|分配|合并|拥挤|层级)|(?:排版|版式|内容比例|页面比例|信息层级|页面层级|视觉平衡|头重脚轻).{0,8}(?:如何|怎么|是否|需不需要|需要吗|安排|设计)|各部分.{0,8}(?:比例|占比)|(?:是否需要|要不要).{0,5}合并.{0,3}页面|哪些内容.{0,8}(?:同一页|哪一页)|哪一页.{0,8}(?:表达形式|放什么|采用什么)|(?:时间线|卡片|图片|图表).{0,8}(?:还是|如何安排|怎么安排|使用什么)|使用.{0,6}(?:什么|哪种)(?:图表|图片|表达形式)|如何.{0,8}(?:保持视觉平衡|避免页面拥挤|避免头重脚轻)|正面.{0,8}负面.{0,8}(?:布局|排布|分布)|结论页.{0,8}(?:布局|排版)/u;
const NO_OPEN_QUESTIONS_VARIANT =
  /^(?:暂无(?:需要用户补充的信息|需确认的问题|补充问题)?|无|没有需要用户补充的信息)[。！!]?$/u;

function normalizeLineNumbering(line: string): string {
  const matches = [...line.matchAll(NUMBERED_ITEM_MARKER)];
  if (matches.length < 2) return line;

  let cursor = 0;
  let normalized = "";
  matches.forEach((match, index) => {
    const markerIndex = match.index ?? 0;
    if (index > 0) {
      normalized += `${line.slice(cursor, markerIndex)}\n`;
      cursor = markerIndex;
    }
  });
  return normalized + line.slice(cursor);
}

/** 仅在同一行出现多个明确编号项时插入换行；不删除、不总结也不改写任何正文。 */
export function normalizePptUnderstandingFormatting(text: string): string {
  return text.replace(/[^\r\n]+/gu, normalizeLineNumbering);
}

export function normalizePptUnderstandingDraftFormatting(
  draft: PptUnderstandingDraft,
): PptUnderstandingDraft {
  const trimmedOpenQuestions = draft.openQuestions.trim();
  const openQuestions =
    !trimmedOpenQuestions || NO_OPEN_QUESTIONS_VARIANT.test(trimmedOpenQuestions)
      ? PPT_NO_OPEN_QUESTIONS_TEXT
      : draft.openQuestions;
  return {
    understandingSummary: normalizePptUnderstandingFormatting(draft.understandingSummary),
    keyPriorities: normalizePptUnderstandingFormatting(draft.keyPriorities),
    narrativeMainline: normalizePptUnderstandingFormatting(draft.narrativeMainline),
    suggestedPageStructure: normalizePptUnderstandingFormatting(draft.suggestedPageStructure),
    visualExpressionAdvice: normalizePptUnderstandingFormatting(draft.visualExpressionAdvice),
    openQuestions: normalizePptUnderstandingFormatting(openQuestions),
  };
}

/** 拒绝把内部读取方式或系统自己的版式决策直接暴露给用户。 */
export function validatePptUnderstandingOutputBoundaries(draft: PptUnderstandingDraft): void {
  const fields = Object.values(draft);
  if (fields.some((value) => INTERNAL_READING_TERM.test(value))) {
    throw new Error("最终理解包含内部读取方式，请重新整理后再输出");
  }
  if (
    draft.openQuestions !== PPT_NO_OPEN_QUESTIONS_TEXT &&
    SYSTEM_PLANNING_QUESTION.test(draft.openQuestions)
  ) {
    throw new Error("仍需确认的问题包含应由系统自动完成的页面或版式决策");
  }
}

export function preparePptUnderstandingDraftForDisplay(
  draft: PptUnderstandingDraft,
): PptUnderstandingDraft {
  const normalized = normalizePptUnderstandingDraftFormatting(draft);
  validatePptUnderstandingOutputBoundaries(normalized);
  return normalized;
}
