import type {
  PptChunkUnderstandingDraft,
  PptMaterialChunk,
  PptUnderstandingDraft,
} from "../types/index.ts";
import {
  buildPptMaterialAnalysisCacheKey,
  mergePptUnderstandingDraftsHierarchically,
} from "./pptUnderstandingRuntime.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const context = {
  topic: "manufacturing report",
  audience: "teachers",
  pageCount: "8 pages",
  style: "clean",
  extraRequirements: "keep numbers",
};
const baseCacheInput = {
  rawMaterial: "# A\n\nfirst paragraph\n\n# B\n\nsecond paragraph",
  modelId: 1,
  modelMaxContextTokens: 8_000,
  reservedOutputTokens: 1_000,
  promptContext: context,
};

const baseKey = buildPptMaterialAnalysisCacheKey(baseCacheInput);
assert(
  baseKey === buildPptMaterialAnalysisCacheKey(baseCacheInput),
  "same material/model/context must produce a stable chunk cache key",
);
assert(
  baseKey !== buildPptMaterialAnalysisCacheKey({ ...baseCacheInput, rawMaterial: `${baseCacheInput.rawMaterial}\nchanged` }),
  "material changes must invalidate chunk cache",
);
assert(
  baseKey !== buildPptMaterialAnalysisCacheKey({ ...baseCacheInput, modelId: 2 }),
  "model changes must invalidate chunk cache",
);
assert(
  baseKey !== buildPptMaterialAnalysisCacheKey({
    ...baseCacheInput,
    promptContext: { ...context, pageCount: "12 pages" },
  }),
  "planning context changes must invalidate chunk cache",
);
assert(!baseKey.includes(baseCacheInput.rawMaterial), "cache key must not contain raw material text");

function makeChunk(index: number): PptMaterialChunk {
  return {
    id: `ppt-material-${index}`,
    index,
    total: 4,
    text: `source ${index}`,
    sourceTitles: [`source title ${index}`],
    headingContext: [`heading ${index}`],
    startCharacter: index * 10,
    endCharacter: index * 10 + 9,
    estimatedTokens: 10,
  };
}

function makeDraft(index: number): PptChunkUnderstandingDraft {
  return {
    chunkId: `ppt-material-${index}`,
    chunkIndex: index,
    understandingSummary: `summary ${index} `.repeat(40),
    keyPriorities: `priority ${index} `.repeat(40),
    narrativeMainline: `mainline ${index} `.repeat(40),
    suggestedPageStructure: `structure ${index} `.repeat(40),
    visualExpressionAdvice: `visual ${index} `.repeat(40),
    openQuestions: `question ${index} `.repeat(40),
  };
}

const chunks = [1, 2, 3, 4].map(makeChunk);
const drafts = [1, 2, 3, 4].map(makeDraft);
const mergeCalls: string[] = [];
const finalDraft: PptUnderstandingDraft = {
  understandingSummary: "final summary",
  keyPriorities: "final priorities",
  narrativeMainline: "final mainline",
  suggestedPageStructure: "final structure",
  visualExpressionAdvice: "final visual",
  openQuestions: "final questions",
};

const merged = await mergePptUnderstandingDraftsHierarchically({
  context,
  chunks,
  drafts,
  modelMaxContextTokens: 4_000,
  reservedOutputTokens: 500,
  mergeDrafts: async (prompt, meta) => {
    mergeCalls.push(`${meta.level}:${meta.batchIndex}/${meta.batchCount}:${prompt.length}`);
    assert(!prompt.includes("source 1"), "merge prompt must not include original raw chunk text");
    return {
      ...finalDraft,
      understandingSummary: `level ${meta.level} batch ${meta.batchIndex}`,
    };
  },
});

assert(mergeCalls.length === merged.mergeRequestCount, "merge request count must match actual merge calls");
assert(mergeCalls.length > 1, "over-budget merge drafts must be merged hierarchically");
assert(
  merged.finalDraft.understandingSummary.includes("level"),
  "hierarchical merge must return the final parsed draft",
);
