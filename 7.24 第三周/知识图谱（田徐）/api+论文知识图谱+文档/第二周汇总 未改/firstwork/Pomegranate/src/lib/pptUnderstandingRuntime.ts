import type {
  PptChunkUnderstandingDraft,
  PptMaterialChunk,
  PptUnderstandingDraft,
} from "@/types";
import {
  PPT_CHUNK_INPUT_SAFETY_RATIO,
  PPT_CHUNK_METADATA_RESERVE_TOKENS,
  PPT_CHUNK_MINIMUM_SAFETY_TOKENS,
  PPT_CHUNK_UNDERSTANDING_OUTPUT_TOKEN_CAP,
} from "./pptMaterialChunking.ts";
import {
  buildPptUnderstandingMergePrompt,
  PPT_CHUNK_UNDERSTANDING_PROMPT_VERSION,
  PPT_MERGE_UNDERSTANDING_PROMPT_VERSION,
  type PptChunkUnderstandingContext,
  type PptUnderstandingMergePromptInput,
} from "./pptChunkUnderstandingPrompt.ts";
import { estimatePptTextTokens, resolvePptReservedOutputTokens } from "./pptContextBudget.ts";
import { PPT_DIRECT_UNDERSTANDING_PROMPT_VERSION } from "./pptUnderstandingPrompt.ts";

const PPT_RUNTIME_CACHE_SCHEMA_VERSION = "ppt-understanding-cache-v2";
const MAX_MERGE_LEVELS = 8;

export interface PptMaterialAnalysisCacheKeyInput {
  rawMaterial: string;
  modelId: number;
  modelMaxContextTokens?: number | null;
  reservedOutputTokens?: number | null;
  promptContext: PptChunkUnderstandingContext;
}

export interface PptMergeBudgetInput {
  modelMaxContextTokens?: number | null;
  reservedOutputTokens?: number | null;
}

interface PptMergeEntry {
  chunkId: string;
  chunkIndex: number;
  sourceTitles: string[];
  headingContext: string[];
  draft: PptChunkUnderstandingDraft;
}

export interface PptHierarchicalMergeInput extends PptMergeBudgetInput {
  context: PptChunkUnderstandingContext;
  chunks: PptMaterialChunk[];
  drafts: PptChunkUnderstandingDraft[];
  mergeDrafts: (prompt: string, meta: {
    level: number;
    batchIndex: number;
    batchCount: number;
  }) => Promise<PptUnderstandingDraft>;
}

export interface PptHierarchicalMergeResult {
  finalDraft: PptUnderstandingDraft;
  mergeRequestCount: number;
}

function stablePptTextHash(value: string): string {
  let hash = 0x811c9dc5;
  for (const char of value) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

function unique(values: string[]): string[] {
  return [...new Set(values.filter(Boolean))];
}

export function buildPptMaterialAnalysisCacheKey(
  input: PptMaterialAnalysisCacheKeyInput,
): string {
  const payload = {
    schema: PPT_RUNTIME_CACHE_SCHEMA_VERSION,
    materialHash: stablePptTextHash(input.rawMaterial),
    materialLength: input.rawMaterial.length,
    modelId: input.modelId,
    modelMaxContextTokens: input.modelMaxContextTokens ?? null,
    reservedOutputTokens: resolvePptReservedOutputTokens(input.reservedOutputTokens),
    promptContext: input.promptContext,
    promptVersions: {
      direct: PPT_DIRECT_UNDERSTANDING_PROMPT_VERSION,
      chunk: PPT_CHUNK_UNDERSTANDING_PROMPT_VERSION,
      merge: PPT_MERGE_UNDERSTANDING_PROMPT_VERSION,
    },
    chunkConfig: {
      inputSafetyRatio: PPT_CHUNK_INPUT_SAFETY_RATIO,
      minimumSafetyTokens: PPT_CHUNK_MINIMUM_SAFETY_TOKENS,
      metadataReserveTokens: PPT_CHUNK_METADATA_RESERVE_TOKENS,
      outputTokenCap: PPT_CHUNK_UNDERSTANDING_OUTPUT_TOKEN_CAP,
    },
  };
  return stablePptTextHash(JSON.stringify(payload));
}

function buildMergeEntries(
  chunks: PptMaterialChunk[],
  drafts: PptChunkUnderstandingDraft[],
): PptMergeEntry[] {
  return chunks.map((chunk) => {
    const draft = drafts.find((item) => item.chunkId === chunk.id);
    if (!draft) {
      throw new Error(`Missing chunk understanding draft: ${chunk.id}`);
    }
    return {
      chunkId: chunk.id,
      chunkIndex: chunk.index,
      sourceTitles: chunk.sourceTitles,
      headingContext: chunk.headingContext,
      draft,
    };
  });
}

function mergePromptFitsBudget(
  context: PptChunkUnderstandingContext,
  entries: PptUnderstandingMergePromptInput["chunks"],
  budget: PptMergeBudgetInput,
): boolean {
  const maxContextTokens = Number.isFinite(budget.modelMaxContextTokens) &&
    (budget.modelMaxContextTokens ?? 0) > 0
    ? Math.floor(budget.modelMaxContextTokens as number)
    : null;
  if (maxContextTokens === null) return true;
  const effectiveInputBudget =
    maxContextTokens - resolvePptReservedOutputTokens(budget.reservedOutputTokens);
  if (effectiveInputBudget <= 0) return false;
  return estimatePptTextTokens(buildPptUnderstandingMergePrompt({ ...context, chunks: entries })) <=
    effectiveInputBudget;
}

function groupMergeEntriesByBudget(
  context: PptChunkUnderstandingContext,
  entries: PptMergeEntry[],
  budget: PptMergeBudgetInput,
): PptMergeEntry[][] {
  const groups: PptMergeEntry[][] = [];
  let current: PptMergeEntry[] = [];
  for (const entry of entries) {
    const candidate = [...current, entry];
    if (candidate.length === 1) {
      if (!mergePromptFitsBudget(context, candidate, budget)) {
        throw new Error(`Merge draft ${entry.chunkIndex} exceeds the current model context budget.`);
      }
      current = candidate;
      continue;
    }
    if (mergePromptFitsBudget(context, candidate, budget)) {
      current = candidate;
      continue;
    }
    groups.push(current);
    current = [entry];
    if (!mergePromptFitsBudget(context, current, budget)) {
      throw new Error(`Merge draft ${entry.chunkIndex} exceeds the current model context budget.`);
    }
  }
  if (current.length > 0) groups.push(current);
  return groups;
}

function buildIntermediateEntry(
  group: PptMergeEntry[],
  draft: PptUnderstandingDraft,
  level: number,
  batchIndex: number,
): PptMergeEntry {
  return {
    chunkId: `ppt-merge-level-${level}-batch-${batchIndex}`,
    chunkIndex: batchIndex,
    sourceTitles: unique(group.flatMap((entry) => entry.sourceTitles)),
    headingContext: unique(group.flatMap((entry) => entry.headingContext)),
    draft: {
      chunkId: `ppt-merge-level-${level}-batch-${batchIndex}`,
      chunkIndex: batchIndex,
      ...draft,
    },
  };
}

export async function mergePptUnderstandingDraftsHierarchically(
  input: PptHierarchicalMergeInput,
): Promise<PptHierarchicalMergeResult> {
  let entries = buildMergeEntries(input.chunks, input.drafts);
  let mergeRequestCount = 0;

  for (let level = 0; level < MAX_MERGE_LEVELS; level += 1) {
    const groups = groupMergeEntriesByBudget(input.context, entries, input);
    if (groups.length === 1) {
      const prompt = buildPptUnderstandingMergePrompt({ ...input.context, chunks: groups[0] });
      mergeRequestCount += 1;
      return {
        finalDraft: await input.mergeDrafts(prompt, {
          level,
          batchIndex: 1,
          batchCount: 1,
        }),
        mergeRequestCount,
      };
    }

    const nextEntries: PptMergeEntry[] = [];
    for (const [index, group] of groups.entries()) {
      const prompt = buildPptUnderstandingMergePrompt({ ...input.context, chunks: group });
      mergeRequestCount += 1;
      const draft = await input.mergeDrafts(prompt, {
        level,
        batchIndex: index + 1,
        batchCount: groups.length,
      });
      nextEntries.push(buildIntermediateEntry(group, draft, level + 1, index + 1));
    }
    entries = nextEntries;
  }

  throw new Error("Unable to merge PPT understanding drafts within the model context budget.");
}
