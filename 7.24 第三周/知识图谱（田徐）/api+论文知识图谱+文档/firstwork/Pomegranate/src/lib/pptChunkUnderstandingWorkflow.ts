import type {
  PptChunkUnderstandingDraft,
  PptMaterialChunk,
  PptUnderstandingDraft,
} from "@/types";

export const MAX_PPT_CHUNK_UNDERSTANDING_CONCURRENCY = 2;

export interface PptChunkUnderstandingWorkflowInput {
  chunks: PptMaterialChunk[];
  cachedDrafts: PptChunkUnderstandingDraft[];
  isCancelled: () => boolean;
  analyzeChunk: (chunk: PptMaterialChunk) => Promise<PptChunkUnderstandingDraft>;
  mergeDrafts: (
    drafts: PptChunkUnderstandingDraft[],
  ) => Promise<PptUnderstandingDraft | PptChunkUnderstandingMergeResult>;
  onChunkStarted?: (chunk: PptMaterialChunk) => void;
  onChunkSucceeded?: (draft: PptChunkUnderstandingDraft) => void;
  onChunkFailed?: (chunk: PptMaterialChunk, error: unknown) => void;
  onMergeStarted?: () => void;
}

export interface PptChunkUnderstandingMergeResult {
  finalDraft: PptUnderstandingDraft;
  requestCount: number;
}

export interface PptChunkUnderstandingWorkflowResult {
  drafts: PptChunkUnderstandingDraft[];
  failedChunkIndexes: number[];
  cancelled: boolean;
  analysisRequestCount: number;
  mergeRequestCount: number;
  finalDraft: PptUnderstandingDraft | null;
}

function isMergeResult(value: PptUnderstandingDraft | PptChunkUnderstandingMergeResult): value is PptChunkUnderstandingMergeResult {
  return "finalDraft" in value && "requestCount" in value;
}

/** 最多并发两个部分；全部成功后只执行一次最终合并，因此总请求严格为 N + 1。 */
export async function executePptChunkUnderstandingWorkflow(
  input: PptChunkUnderstandingWorkflowInput,
): Promise<PptChunkUnderstandingWorkflowResult> {
  const draftsById = new Map(input.cachedDrafts.map((draft) => [draft.chunkId, draft]));
  const missingChunks = input.chunks.filter((chunk) => !draftsById.has(chunk.id));
  const failedChunkIndexes: number[] = [];
  let analysisRequestCount = 0;
  let cursor = 0;

  const worker = async () => {
    while (!input.isCancelled()) {
      const chunk = missingChunks[cursor];
      cursor += 1;
      if (!chunk) return;
      analysisRequestCount += 1;
      input.onChunkStarted?.(chunk);
      try {
        const draft = await input.analyzeChunk(chunk);
        if (input.isCancelled()) return;
        draftsById.set(draft.chunkId, draft);
        input.onChunkSucceeded?.(draft);
      } catch (error) {
        if (input.isCancelled()) return;
        failedChunkIndexes.push(chunk.index);
        input.onChunkFailed?.(chunk, error);
      }
    }
  };

  await Promise.all(
    Array.from(
      { length: Math.min(MAX_PPT_CHUNK_UNDERSTANDING_CONCURRENCY, missingChunks.length) },
      () => worker(),
    ),
  );

  const drafts = input.chunks
    .map((chunk) => draftsById.get(chunk.id))
    .filter((draft): draft is PptChunkUnderstandingDraft => draft !== undefined)
    .sort((left, right) => left.chunkIndex - right.chunkIndex);
  const cancelled = input.isCancelled();
  const uniqueFailedIndexes = [...new Set(failedChunkIndexes)].sort((left, right) => left - right);
  if (cancelled || uniqueFailedIndexes.length > 0 || drafts.length !== input.chunks.length) {
    return {
      drafts,
      failedChunkIndexes: uniqueFailedIndexes,
      cancelled,
      analysisRequestCount,
      mergeRequestCount: 0,
      finalDraft: null,
    };
  }

  input.onMergeStarted?.();
  const mergeResult = await input.mergeDrafts(drafts);
  const finalDraft = isMergeResult(mergeResult) ? mergeResult.finalDraft : mergeResult;
  return {
    drafts,
    failedChunkIndexes: [],
    cancelled: false,
    analysisRequestCount,
    mergeRequestCount: isMergeResult(mergeResult) ? mergeResult.requestCount : 1,
    finalDraft,
  };
}
