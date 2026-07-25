import type { PptMaterialChunk, PptMaterialChunkPlan } from "@/types";
import {
  buildPptChunkUnderstandingPromptParts,
  getPptChunkUnderstandingBudgetParts,
  type PptChunkUnderstandingContext,
} from "./pptChunkUnderstandingPrompt.ts";
import {
  calculatePptContextBudget,
  estimatePptTextTokens,
  resolvePptReservedOutputTokens,
  type PptContextBudgetStatus,
} from "./pptContextBudget.ts";

export const PPT_CHUNK_INPUT_SAFETY_RATIO = 0.1;
export const PPT_CHUNK_MINIMUM_SAFETY_TOKENS = 256;
export const PPT_CHUNK_METADATA_RESERVE_TOKENS = 128;
export const PPT_CHUNK_UNDERSTANDING_OUTPUT_TOKEN_CAP = 4096;
const MINIMUM_CHUNK_MATERIAL_TOKENS = 128;
const MAX_BUDGET_REFINEMENT_PASSES = 8;

interface TextRange {
  start: number;
  end: number;
}

interface SourceRange extends TextRange {
  title: string | null;
}

export interface PlanPptMaterialChunksInput {
  rawMaterial: string;
  modelMaxContextTokens?: number | null;
  reservedOutputTokens?: number | null;
  promptContext: PptChunkUnderstandingContext;
}

export interface PptMaterialRequestPlan {
  mode: "direct" | "chunked";
  chunkRequests: number;
  finalUnderstandingRequests: 1;
  minimumTotalRequests: number;
  requiresFeeConfirmation: boolean;
}

export function resolvePptMaterialRequestPlan(input: {
  contextStatus: PptContextBudgetStatus;
  totalChunks?: number;
  cachedChunks?: number;
}): PptMaterialRequestPlan {
  if (input.contextStatus === "unknown") {
    throw new Error("当前模型未配置上下文长度，无法安全规划素材请求。");
  }
  if (input.contextStatus !== "exceeded") {
    return {
      mode: "direct",
      chunkRequests: 0,
      finalUnderstandingRequests: 1,
      minimumTotalRequests: 1,
      requiresFeeConfirmation: false,
    };
  }
  const chunkRequests = Math.max(0, (input.totalChunks ?? 0) - (input.cachedChunks ?? 0));
  return {
    mode: "chunked",
    chunkRequests,
    finalUnderstandingRequests: 1,
    minimumTotalRequests: chunkRequests + 1,
    requiresFeeConfirmation: chunkRequests > 0 && (input.cachedChunks ?? 0) === 0,
  };
}

function unique(values: string[]): string[] {
  return [...new Set(values.filter(Boolean))];
}

function extractSourceTitle(header: string): string | null {
  const dividerIndex = header.indexOf("｜");
  if (dividerIndex >= 0) return header.slice(dividerIndex + 1).trim() || null;
  return header.replace(/^#\s*来源\s*\d+\s*：?/u, "").trim() || null;
}

function findSourceRanges(text: string): SourceRange[] {
  const matches = [...text.matchAll(/^#\s*来源\s*\d+\s*：[^\r\n]*$/gmu)];
  if (matches.length === 0) return [{ start: 0, end: text.length, title: null }];

  const ranges: SourceRange[] = [];
  const firstStart = matches[0]?.index ?? 0;
  if (firstStart > 0) ranges.push({ start: 0, end: firstStart, title: null });
  matches.forEach((match, index) => {
    const start = match.index ?? 0;
    const end = matches[index + 1]?.index ?? text.length;
    ranges.push({ start, end, title: extractSourceTitle(match[0]) });
  });
  return ranges;
}

function boundaryPositions(
  text: string,
  range: TextRange,
  pattern: RegExp,
  positionForMatch: (match: RegExpMatchArray) => number,
): number[] {
  return [...text.slice(range.start, range.end).matchAll(pattern)]
    .map((match) => range.start + positionForMatch(match))
    .filter((position) => position > range.start && position < range.end);
}

function headingBoundaries(text: string, range: TextRange): number[] {
  return boundaryPositions(text, range, /^#{1,3}\s+[^\r\n]+$/gmu, (match) => match.index ?? 0);
}

function paragraphBoundaries(text: string, range: TextRange): number[] {
  return boundaryPositions(
    text,
    range,
    /\r?\n[\t ]*\r?\n+/gu,
    (match) => (match.index ?? 0) + match[0].length,
  );
}

function listGroupBoundaries(text: string, range: TextRange): number[] {
  const local = text.slice(range.start, range.end);
  const lines = [...local.matchAll(/^.*(?:\r?\n|$)/gmu)].filter((match) => match[0].length > 0);
  const positions: number[] = [];
  let previousWasList = false;
  for (const line of lines) {
    const currentWasList = /^\s*(?:[-+*]|\d+[.)]|>|\[[ xX]\])\s+/u.test(line[0]);
    const lineStart = range.start + (line.index ?? 0);
    if (currentWasList !== previousWasList && lineStart > range.start) positions.push(lineStart);
    if (previousWasList && !currentWasList) positions.push(lineStart);
    previousWasList = currentWasList;
  }
  return unique(positions.map(String)).map(Number).filter((position) => position < range.end);
}

function sentenceBoundaries(text: string, range: TextRange): number[] {
  return boundaryPositions(
    text,
    range,
    /[。！？.!?；;](?:[”’」』】）)]*)[\t ]*/gu,
    (match) => (match.index ?? 0) + match[0].length,
  );
}

function splitRangeAt(range: TextRange, positions: number[]): TextRange[] {
  const sorted = [...new Set(positions)].sort((left, right) => left - right);
  const result: TextRange[] = [];
  let start = range.start;
  for (const position of sorted) {
    if (position <= start || position >= range.end) continue;
    result.push({ start, end: position });
    start = position;
  }
  if (start < range.end) result.push({ start, end: range.end });
  return result;
}

function hardSplitRange(text: string, range: TextRange, maxTokens: number): TextRange[] {
  const result: TextRange[] = [];
  let start = range.start;
  while (start < range.end) {
    const characters = Array.from(text.slice(start, range.end));
    let low = 1;
    let high = characters.length;
    let bestCharacterCount = 1;
    while (low <= high) {
      const middle = Math.floor((low + high) / 2);
      const candidate = characters.slice(0, middle).join("");
      if (estimatePptTextTokens(candidate) <= maxTokens) {
        bestCharacterCount = middle;
        low = middle + 1;
      } else {
        high = middle - 1;
      }
    }
    const end = start + characters.slice(0, bestCharacterCount).join("").length;
    result.push({ start, end });
    start = end;
  }
  return result;
}

const STRUCTURAL_BOUNDARIES = [headingBoundaries, paragraphBoundaries, listGroupBoundaries, sentenceBoundaries];

function splitOversizedRange(
  text: string,
  range: TextRange,
  maxTokens: number,
  boundaryLevel = 0,
): TextRange[] {
  if (estimatePptTextTokens(text.slice(range.start, range.end)) <= maxTokens) return [range];
  if (boundaryLevel >= STRUCTURAL_BOUNDARIES.length) {
    return hardSplitRange(text, range, maxTokens);
  }
  const positions = STRUCTURAL_BOUNDARIES[boundaryLevel](text, range);
  if (positions.length === 0) {
    return splitOversizedRange(text, range, maxTokens, boundaryLevel + 1);
  }
  return splitRangeAt(range, positions).flatMap((part) =>
    splitOversizedRange(text, part, maxTokens, boundaryLevel + 1),
  );
}

function sourceTitlesForRange(sourceRanges: SourceRange[], range: TextRange): string[] {
  return unique(
    sourceRanges
      .filter((source) => source.end > range.start && source.start < range.end)
      .map((source) => source.title ?? ""),
  );
}

function headingContextForRange(text: string, sourceRanges: SourceRange[], range: TextRange): string[] {
  const headings: string[] = [];
  for (const source of sourceRanges) {
    if (source.end <= range.start || source.start >= range.end) continue;
    const headingCountBeforeSource = headings.length;
    const scanEnd = Math.min(source.end, range.end);
    const stack: string[] = [];
    for (const match of text.slice(source.start, scanEnd).matchAll(/^(#{1,3})\s+([^\r\n]+)$/gmu)) {
      const absoluteStart = source.start + (match.index ?? 0);
      const fullHeading = match[0];
      if (/^#\s*来源\s*\d+\s*：/u.test(fullHeading)) continue;
      const level = match[1].length;
      const title = match[2].trim();
      stack[level - 1] = title;
      stack.length = level;
      if (absoluteStart < range.start) continue;
      headings.push(...stack);
    }
    if (headings.length === headingCountBeforeSource && stack.length > 0) headings.push(...stack);
  }
  return unique(headings);
}

function buildChunks(text: string, maxTokens: number): PptMaterialChunk[] {
  const sourceRanges = findSourceRanges(text);
  const structuralPieces = sourceRanges.flatMap((source) =>
    splitOversizedRange(text, source, maxTokens),
  );
  const packed: TextRange[] = [];
  let current: TextRange | null = null;
  for (const piece of structuralPieces) {
    if (!current) {
      current = { ...piece };
      continue;
    }
    const combined = text.slice(current.start, piece.end);
    if (estimatePptTextTokens(combined) <= maxTokens) {
      current.end = piece.end;
    } else {
      packed.push(current);
      current = { ...piece };
    }
  }
  if (current) packed.push(current);

  const total = packed.length;
  const chunks = packed.map((range, zeroBasedIndex): PptMaterialChunk => {
    const chunkText = text.slice(range.start, range.end);
    return {
      id: `ppt-material-${zeroBasedIndex + 1}`,
      index: zeroBasedIndex + 1,
      total,
      text: chunkText,
      sourceTitles: sourceTitlesForRange(sourceRanges, range),
      headingContext: headingContextForRange(text, sourceRanges, range),
      startCharacter: range.start,
      endCharacter: range.end,
      estimatedTokens: estimatePptTextTokens(chunkText),
    };
  });
  if (chunks.map((chunk) => chunk.text).join("") !== text) {
    throw new Error("素材分块未能保持原始顺序和内容");
  }
  return chunks;
}

export function planPptMaterialChunks(input: PlanPptMaterialChunksInput): PptMaterialChunkPlan {
  if (!input.rawMaterial.trim()) throw new Error("当前没有可分析的 PPT 素材");
  const maxContextTokens = Number.isFinite(input.modelMaxContextTokens) &&
    (input.modelMaxContextTokens ?? 0) > 0
    ? Math.floor(input.modelMaxContextTokens as number)
    : null;
  if (maxContextTokens === null) {
    throw new Error("当前模型未配置上下文长度，无法安全规划长素材分块，请先完善模型配置。");
  }
  const outputReserveTokens = Math.min(
    resolvePptReservedOutputTokens(input.reservedOutputTokens),
    PPT_CHUNK_UNDERSTANDING_OUTPUT_TOKEN_CAP,
  );
  const promptParts = getPptChunkUnderstandingBudgetParts(input.promptContext);
  const emptyBudget = calculatePptContextBudget({
    modelMaxContextTokens: maxContextTokens,
    rawMaterial: "",
    promptText: promptParts.promptText,
    metadataText: promptParts.metadataText,
    reservedOutputTokens: outputReserveTokens,
  });
  const promptOverheadTokens = emptyBudget.estimatedPromptTokens;
  const metadataReserveTokens =
    emptyBudget.estimatedMetadataTokens + PPT_CHUNK_METADATA_RESERVE_TOKENS;
  const effectiveInputBudget = emptyBudget.effectiveInputBudget ?? 0;
  const safetyMarginTokens = Math.max(
    PPT_CHUNK_MINIMUM_SAFETY_TOKENS,
    Math.floor(effectiveInputBudget * PPT_CHUNK_INPUT_SAFETY_RATIO),
  );
  let chunkTokenBudget =
    effectiveInputBudget -
    promptOverheadTokens -
    metadataReserveTokens -
    safetyMarginTokens;
  if (chunkTokenBudget < MINIMUM_CHUNK_MATERIAL_TOKENS) {
    throw new Error("当前模型在预留输出与安全余量后没有足够的分块输入容量，请调整模型配置。");
  }

  let chunks: PptMaterialChunk[] = [];
  for (let pass = 0; pass < MAX_BUDGET_REFINEMENT_PASSES; pass += 1) {
    chunks = buildChunks(input.rawMaterial, chunkTokenBudget);
    const largestOverflow = chunks.reduce((overflow, chunk) => {
      const estimatedRequestTokens = estimatePptTextTokens(
        buildPptChunkUnderstandingPromptParts(input.promptContext, chunk).fullPrompt,
      );
      return Math.max(overflow, estimatedRequestTokens - effectiveInputBudget);
    }, 0);
    if (largestOverflow <= 0) break;
    chunkTokenBudget -= largestOverflow + 64;
    if (chunkTokenBudget < MINIMUM_CHUNK_MATERIAL_TOKENS) {
      throw new Error("分块元数据占用过大，当前模型无法安全处理该素材。");
    }
  }
  if (
    chunks.some(
      (chunk) =>
        estimatePptTextTokens(
          buildPptChunkUnderstandingPromptParts(input.promptContext, chunk).fullPrompt,
        ) >
        effectiveInputBudget,
    )
  ) {
    throw new Error("无法在当前模型上下文内生成安全分块计划。");
  }

  return {
    chunks,
    totalCharacters: input.rawMaterial.length,
    totalEstimatedTokens: estimatePptTextTokens(input.rawMaterial),
    chunkTokenBudget,
    promptOverheadTokens,
    metadataReserveTokens,
    outputReserveTokens,
    modelMaxContextTokens: maxContextTokens,
  };
}
